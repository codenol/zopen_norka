//! The canvas' marks: which sections no longer match what they were built from.
//!
//! The panel answers that for the section somebody selected, one read at a time
//! (see `section_sync_requests`). This is the whole-document half: the list of
//! sections, then one digest per referenced analytics asset, one request per
//! tick, published to the canvas on a slow cadence. The thread-local bookkeeping
//! it runs on lives in the spine.

use std::cell::RefCell;
use std::rc::Rc;

use super::answer::{self, MarkAnswer};
use super::{
    MarkPending, MARKS_KEY, MARKS_LIST_IN_FLIGHT, MARKS_PENDING, MARKS_REFRESH_TICKS, MARKS_TICKS,
    MARK_DIGESTS,
};
use op_editor_core::section::{
    louder, mockup_fingerprint, refused_link_state, unchecked_link_state, LinkState, SectionDigest,
    SectionProperties,
};
use op_editor_core::{section_routes, NodeId};

use crate::live_sync;
use crate::repaint_ctx::RepaintContext;

/// Keep the canvas' marks current: which sections no longer match what they
/// were built from.
///
/// The panel answers that for the section somebody selected, one read at a
/// time. This answers it for the whole document — the list of sections, then
/// one digest per referenced analytics asset, one request per tick — because
/// the canvas has to mark sections nobody has clicked on.
///
/// A different document clears the marks rather than carrying them over: a node
/// id means nothing outside the file it came from, and a mark left standing
/// from the previous document would accuse a section that is perfectly fine.
pub(super) fn refresh_marks<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
    let (key, document) = {
        let Ok(context) = inner.try_borrow() else {
            return;
        };
        let state = context.host().editor_state();
        (state.editor_ui.file_key.clone(), state.doc.clone())
    };
    let changed_document = MARKS_KEY.with(|slot| *slot.borrow() != key);
    if changed_document {
        MARKS_KEY.with(|slot| *slot.borrow_mut() = key.clone());
        MARKS_PENDING.with(|slot| slot.borrow_mut().clear());
        MARK_DIGESTS.with(|slot| slot.borrow_mut().clear());
        MARKS_LIST_IN_FLIGHT.with(|slot| slot.set(false));
        write_marks(inner, Vec::new());
    }
    let Some(key) = key else {
        return;
    };
    // One link at a time, in the order the sections were listed.
    let next = MARKS_PENDING.with(|slot| {
        let mut pending = slot.borrow_mut();
        let Some(index) = pending
            .iter()
            .position(|section| section.links.iter().any(|link| !has_digest(&link.key)))
        else {
            return None;
        };
        let section = &pending[index];
        let asset = section
            .links
            .iter()
            .find(|link| !has_digest(&link.key))?
            .key
            .clone();
        Some((index, asset))
    });
    let Some((index, asset)) = next else {
        // Everything resolved: publish what they add up to, and start the next
        // round on the slow cadence rather than every tick.
        finish_marks(inner, &document);
        let ticks = MARKS_TICKS.with(|slot| {
            let next = slot.get().wrapping_add(1);
            slot.set(next);
            next
        });
        let due = ticks % MARKS_REFRESH_TICKS == 0 || changed_document;
        if due
            && !MARKS_LIST_IN_FLIGHT.with(|slot| slot.get())
            && MARKS_PENDING.with(|slot| slot.borrow().is_empty())
        {
            start_marks_list(inner, base, &key);
        }
        return;
    };
    let _ = index;
    read_mark_digest(inner, base, &asset, &document);
}

/// Whether this asset's digest has been asked for already.
fn has_digest(asset: &str) -> bool {
    MARK_DIGESTS.with(|slot| slot.borrow().iter().any(|(known, _)| known == asset))
}

/// Ask the store what one analytics document hashes to now.
fn read_mark_digest<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    base: &str,
    asset: &str,
    document: &jian_ops_schema::PenDocument,
) {
    MARK_DIGESTS.with(|slot| {
        slot.borrow_mut()
            .push((asset.to_string(), MarkAnswer::Asked))
    });
    let url = format!("{base}{}", section_routes::analytics(asset));
    let inner_for_reply = inner.clone();
    let asset = asset.to_string();
    let document = document.clone();
    let _ = live_sync::get_with_status(
        &url,
        Rc::new(move |status, body| {
            // A 403 is a refusal, a 404 is a deletion, and anything else — a
            // 5xx, a status this build does not know, a body with no digest —
            // is a check that did not complete. Reading the last of those as
            // "no digest" would put the octagon — "what this was built from is
            // gone" — on a section whose store merely failed to answer
            // (#145, one status code over from #110).
            let answer = answer::mark_answer(status, &body);
            MARK_DIGESTS.with(|slot| {
                for entry in slot.borrow_mut().iter_mut() {
                    if entry.0 == asset {
                        entry.1 = answer.clone();
                    }
                }
            });
            finish_marks(&inner_for_reply, &document);
        }),
    );
}

/// Ask which sections the document has.
fn start_marks_list<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str, key: &str) {
    MARKS_LIST_IN_FLIGHT.with(|slot| slot.set(true));
    let url =
        crate::daemon_base::with_tenant_param(&format!("{base}{}", section_routes::sections(key)));
    let inner_for_reply = inner.clone();
    let _ = live_sync::get_with_status(
        &url,
        Rc::new(move |status, body| {
            MARKS_LIST_IN_FLIGHT.with(|slot| slot.set(false));
            if status >= 400 {
                return;
            }
            let Some(rows) = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| value.get("sections")?.as_array().cloned())
            else {
                return;
            };
            // The screens' half of each link comes from the document this tab
            // holds: the store knows the analytics, not the section's children.
            let Ok(context) = inner_for_reply.try_borrow() else {
                return;
            };
            let state = context.host().editor_state();
            let mut pending = Vec::new();
            for row in rows {
                let Some(node) = row
                    .get("nodeId")
                    .and_then(|node| node.as_str())
                    .and_then(op_editor_core::NodeId::new_opt)
                else {
                    continue;
                };
                let Some(mockups) = mockup_digest_of(&state.doc, &node) else {
                    continue;
                };
                let links = row
                    .get("properties")
                    .cloned()
                    .and_then(|properties| {
                        serde_json::from_value::<SectionProperties>(properties).ok()
                    })
                    .map(|properties| properties.analytics)
                    .unwrap_or_default();
                if !links.is_empty() {
                    pending.push(MarkPending {
                        node,
                        mockups,
                        links,
                    });
                }
            }
            drop(context);
            MARKS_PENDING.with(|slot| *slot.borrow_mut() = pending);
            MARK_DIGESTS.with(|slot| slot.borrow_mut().clear());
        }),
    );
}

/// The mockup fingerprint of one section, from the document.
///
/// The pages are walked rather than the scene: a mark has to survive a section
/// being scrolled out of view, and the scene is built from what is on screen.
fn mockup_digest_of(
    document: &jian_ops_schema::PenDocument,
    node: &op_editor_core::NodeId,
) -> Option<SectionDigest> {
    for page in document.pages.iter().flatten() {
        if let Some(found) = op_editor_core::walkers::find_node(&page.children, node) {
            return Some(mockup_fingerprint(found));
        }
    }
    None
}

/// Turn what has been resolved into the marks the canvas paints.
fn finish_marks<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    document: &jian_ops_schema::PenDocument,
) {
    let pending = MARKS_PENDING.with(|slot| slot.borrow().clone());
    if pending.is_empty() {
        return;
    }
    let mut marks = Vec::new();
    for section in &pending {
        let mut worst: Option<LinkState> = None;
        for link in &section.links {
            let answer = MARK_DIGESTS.with(|slot| {
                slot.borrow()
                    .iter()
                    .find(|(known, _)| known == &link.key)
                    .map(|(_, answer)| answer.clone())
            });
            // Nobody has looked this asset up yet, so nothing is established
            // about the link and it is not marked.
            let Some(answer) = answer else {
                continue;
            };
            // The shared rule: the link against what the store holds now and
            // what the screens are now. The browser and the panel answer by the
            // same function rather than by two that agree today — including the
            // case where the store refused this reader, which is a fact about
            // the reader rather than about the asset, and the case where the
            // check did not complete, which is a fact about neither.
            let state = match answer {
                MarkAnswer::Asked => continue,
                MarkAnswer::Refused => refused_link_state(Some(link)),
                MarkAnswer::Failed => unchecked_link_state(Some(link)),
                MarkAnswer::Answered(current) => op_editor_core::section::link_state(
                    Some(link),
                    current.as_ref(),
                    &section.mockups,
                ),
            };
            if state.is_in_sync() {
                continue;
            }
            // A missing asset outranks a refused one, which outranks one whose
            // check did not complete, which outranks a drifted one: the canvas
            // has one glyph to say it with, and the ordering is the model's
            // rather than this file's (`louder`).
            worst = Some(louder(worst, state));
        }
        if let Some(state) = worst {
            marks.push((section.node.clone(), state));
        }
    }
    let _ = document;
    write_marks(inner, marks);
}

/// Put the marks where the canvas reads them.
fn write_marks<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    marks: Vec<(NodeId, LinkState)>,
) {
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    let state = context.host_mut().editor_state_mut();
    if state.editor_ui.section_marks.len() == marks.len() {
        // Cheap equality: the same set of sections in the same states is the
        // common case every 250 ms, and a repaint per tick is not free.
        let unchanged = marks
            .iter()
            .all(|(node, mark)| state.editor_ui.section_marks.of(node) == Some(*mark));
        if unchanged {
            return;
        }
    }
    state.editor_ui.section_marks.replace(marks);
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

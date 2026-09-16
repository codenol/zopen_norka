//! Read the selected section's properties (#59).
//!
//! The Section block shows values that are NOT in the document — the analytics a
//! section was built from, its summary, its flows live in the daemon's store,
//! because a `.op` file cannot carry them (see `op_editor_core::section`) — so
//! something has to go and ask. This is that something.
//!
//! Three jobs per tick, in this order:
//!
//! 1. watch the selection and tell the panel which section it is looking at;
//! 2. when that section changes, read `GET /api/files/<key>/sections/<node>`;
//! 3. resolve each attached analytics document against `GET /api/analytics/<key>`
//!    so the block can say whether it still hashes to what the link recorded —
//!    the one claim in the block that can quietly become false.
//!
//! ## Why the mockup fingerprint is taken here
//!
//! A link is one claim about two things: the analytics it was made from, and the
//! screens that stood there when it was made. The second half is in the document
//! this tab is holding, so it is computed locally (`mockup_fingerprint`) at the
//! moment the properties arrive; the first half needs the store and comes back
//! over the wire. Both then go to the shared
//! [`link_state`](op_editor_core::section::link_state), so the browser and any
//! other reader of the same link answer by one rule rather than two.
//!
//! ## Why one request at a time
//!
//! A section may reference several analytics documents. They are resolved one
//! per tick, in the order the section lists them: this is a panel somebody is
//! looking at, not a dashboard, and a burst of requests is a burst nobody asked
//! for.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use op_editor_core::editor_ui_state::section_panel::SectionLink;
use op_editor_core::section::{
    link_state, louder, mockup_fingerprint, refused_link_state, unchecked_link_state, LinkState,
    SectionDigest, SectionProperties,
};
use op_editor_core::{section, section_routes, NodeId};

// How a status code becomes a fact about an asset, for both readers (#145).
#[path = "section_sync_answer.rs"]
mod answer;
#[cfg(test)]
#[path = "section_sync_tests.rs"]
mod tests;

use answer::{MarkAnswer, Resolution};

use crate::dom_io::{open_file_picker, read_file, ReadMode};
use crate::live_sync;
use crate::repaint_ctx::RepaintContext;

/// Tick cadence: fast enough that selecting a section feels answered, slow
/// enough that an idle editor costs nothing.
const TICK_MS: i32 = 250;

/// One answer, on its way to the panel.
struct Pending {
    node: NodeId,
    properties: SectionProperties,
    /// The section's screens as they are now: the second half of every link.
    mockups: SectionDigest,
    resolved: Vec<(String, Resolution)>,
}

thread_local! {
    /// The properties that arrived and are waiting for their links.
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    /// The section a properties request is in flight for.
    static IN_FLIGHT: RefCell<Option<NodeId>> = const { RefCell::new(None) };
    /// A write is in flight. One at a time: two saves racing would land in
    /// whichever order the network chose.
    static SAVE_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
    /// The document whose sections the canvas marks are about.
    static MARKS_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Sections whose links are being resolved for the canvas marks.
    static MARKS_PENDING: RefCell<Vec<MarkPending>> = const { RefCell::new(Vec::new()) };
    /// Digests already asked for, keyed by asset — a document may reference the
    /// same analytics from several sections.
    static MARK_DIGESTS: RefCell<Vec<(String, MarkAnswer)>> = const { RefCell::new(Vec::new()) };
    /// Whether a list of sections is on its way.
    static MARKS_LIST_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
    /// Ticks since the marks were last refreshed. The canvas is told about a
    /// drift every couple of seconds rather than on every frame: the store is
    /// somewhere else, and a document whose sections are in sync should cost
    /// nothing to look at.
    static MARKS_TICKS: Cell<u32> = const { Cell::new(0) };
}

/// How many ticks between two refreshes of the canvas marks.
const MARKS_REFRESH_TICKS: u32 = 8;

/// One section the canvas is waiting to mark.
#[derive(Clone)]
struct MarkPending {
    node: NodeId,
    /// The mockup digest of THIS section's screens, taken when its properties
    /// arrived.
    mockups: SectionDigest,
    /// The links it carries, in order.
    links: Vec<op_editor_core::section::AnalyticsLink>,
}

/// Wire the section reader onto the mounted shell. Called from mount.
pub(crate) fn start<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let base = crate::daemon_base::daemon_base();
    let inner = inner.clone();
    let tick: Rc<dyn Fn()> = Rc::new(move || {
        watch_selection(&inner);
        attach_requested(&inner, &base);
        save_pending(&inner, &base);
        resolve_next(&inner, &base);
        refresh_marks(&inner, &base);
    });
    let _ = live_sync::start_interval(TICK_MS, tick);
}

/// The selected node, its document key, and whether it is a section.
fn selection<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
) -> (Option<NodeId>, Option<String>, bool) {
    let Ok(context) = inner.try_borrow() else {
        return (None, None, false);
    };
    let state = context.host().editor_state();
    let is_section = state.selected_node().is_some_and(section::is_section);
    // `NodeId::NONE` is how "nothing is selected" is spelled in the editor.
    let anchor = state.selection.anchor.clone();
    (
        anchor.is_real().then_some(anchor),
        state.editor_ui.file_key.clone(),
        is_section,
    )
}

/// Tell the panel which section it is looking at, and start a read when it
/// changes.
fn watch_selection<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let (anchor, key, is_section) = selection(inner);
    // Only a section has properties; every other selection is "nothing to show"
    // and must clear the block rather than leave the last section's summary on
    // screen under a different node's name.
    let watched = anchor.filter(|_| is_section);
    let (changed, answered) = {
        let Ok(context) = inner.try_borrow() else {
            return;
        };
        let panel = &context.host().editor_state().editor_ui.section_panel;
        (panel.node != watched, panel.read)
    };
    if changed {
        let Ok(mut context) = inner.try_borrow_mut() else {
            return;
        };
        context
            .host_mut()
            .editor_state_mut()
            .editor_ui
            .section_panel
            .select(watched.clone());
        context.host_mut().mark_editor_state_dirty();
        let _ = context.repaint();
    }
    let (Some(node), Some(key)) = (watched, key) else {
        return;
    };
    // Already answered, still being asked, or waiting on its links.
    if answered || IN_FLIGHT.with(|slot| slot.borrow().is_some()) {
        return;
    }
    start_properties(inner, &key, node);
}

/// Ask the daemon what this section carries.
fn start_properties<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, key: &str, node: NodeId) {
    let url = crate::daemon_base::with_tenant_param(&format!(
        "{}{}",
        crate::daemon_base::daemon_base(),
        section_routes::section(key, node.as_str())
    ));
    IN_FLIGHT.with(|slot| *slot.borrow_mut() = Some(node.clone()));
    let reply_inner = inner.clone();
    let reply_node = node.clone();
    let started = live_sync::get_with_status(
        &url,
        Rc::new(move |status, body| {
            IN_FLIGHT.with(|slot| *slot.borrow_mut() = None);
            apply_properties(&reply_inner, &reply_node, status, &body);
        }),
    );
    if !started {
        IN_FLIGHT.with(|slot| *slot.borrow_mut() = None);
    }
}

/// The daemon answered: keep the properties, and note the links still to
/// resolve.
fn apply_properties<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    node: &NodeId,
    status: u16,
    body: &str,
) {
    let properties = (status < 400)
        .then(|| serde_json::from_str::<serde_json::Value>(body).ok())
        .flatten()
        .and_then(|value| value.get("properties").cloned())
        .and_then(|value| serde_json::from_value::<SectionProperties>(value).ok());
    let Some(properties) = properties else {
        fail(inner, node);
        return;
    };
    // The screens' half of every link, computed from the document this tab
    // holds — and only while the selection is still the section we asked about,
    // because a fingerprint of somebody else's screens would mark every link
    // moved.
    let Ok(context) = inner.try_borrow() else {
        return;
    };
    let state = context.host().editor_state();
    if &state.selection.anchor != node {
        return;
    }
    let Some(selected) = state.selected_node() else {
        return;
    };
    let mockups = mockup_fingerprint(selected);
    drop(context);

    let resolved = properties
        .analytics
        .iter()
        .map(|link| (link.key.clone(), Resolution::Waiting))
        .collect();
    PENDING.with(|slot| {
        *slot.borrow_mut() = Some(Pending {
            node: node.clone(),
            properties,
            mockups,
            resolved,
        })
    });
}

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
fn refresh_marks<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
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

/// The panel asked for an analytics document: open the file dialog, read what
/// the person picks, load it into the store, and attach what comes back.
///
/// The order is forced by where the facts live. The store issues the KEY and
/// knows the DIGEST of the markdown it just took; the document in this tab has
/// the section's own screens, which is the other half of the link. Neither side
/// can build the link alone, and this function is the one place both are in
/// hand.
fn attach_requested<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) -> bool {
    let wanted = {
        let Ok(mut context) = inner.try_borrow_mut() else {
            return false;
        };
        context
            .host_mut()
            .editor_state_mut()
            .editor_ui
            .section_panel
            .take_analytics_file_request()
    };
    if !wanted {
        return false;
    }
    let inner_for_file = inner.clone();
    let base = base.to_string();
    open_file_picker(
        ".md,.markdown,.txt,text/markdown",
        Box::new(move |file| {
            let name = file.name();
            let inner_for_read = inner_for_file.clone();
            let base = base.clone();
            read_file(
                file,
                ReadMode::Text,
                Box::new(move |value| match value.as_string() {
                    Some(markdown) => create_asset(&inner_for_read, &base, &name, &markdown),
                    None => finish_load(&inner_for_read),
                }),
            );
        }),
    );
    true
}

/// Load the markdown into the store, then attach what it answers.
fn create_asset<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    base: &str,
    file_name: &str,
    markdown: &str,
) {
    // The name a person reads is the file's, without the extension: "checkout.md"
    // is an analytics document called "checkout", and the extension is the
    // store's business rather than the section's.
    let name = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or(file_name)
        .to_string();
    let body = serde_json::json!({ "name": name, "markdown": markdown }).to_string();
    let url = format!("{base}{}", section_routes::ANALYTICS);
    let inner_for_reply = inner.clone();
    let started = live_sync::post_json_with_status(
        &url,
        &body,
        Rc::new(move |status, response| {
            if status >= 400 {
                finish_load(&inner_for_reply);
                return;
            }
            let created = serde_json::from_str::<serde_json::Value>(&response)
                .ok()
                .and_then(|value| {
                    let asset = value.get("asset")?;
                    Some((
                        asset.get("key")?.as_str()?.to_string(),
                        asset
                            .get("name")
                            .and_then(|name| name.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    ))
                });
            let Some((key, name)) = created else {
                finish_load(&inner_for_reply);
                return;
            };
            read_digest(&inner_for_reply, base_for(&inner_for_reply), &key, &name);
        }),
    );
    if !started {
        finish_load(inner);
    }
}

/// The digest of the markdown as the store has it — the half of the link only
/// the store can answer.
fn read_digest<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    base: String,
    key: &str,
    name: &str,
) {
    let url = format!("{base}{}", section_routes::analytics(key));
    let inner_for_reply = inner.clone();
    let key = key.to_string();
    let name = name.to_string();
    let _ = live_sync::get_with_status(
        &url,
        Rc::new(move |status, body| {
            let digest = (status < 400)
                .then(|| serde_json::from_str::<serde_json::Value>(&body).ok())
                .flatten()
                .and_then(|value| value.get("digest")?.as_str().map(SectionDigest::of_hex));
            let Some(digest) = digest else {
                finish_load(&inner_for_reply);
                return;
            };
            attach(&inner_for_reply, &key, &name, digest);
        }),
    );
}

/// Build the link from both halves and queue the write that makes it real.
fn attach<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    key: &str,
    name: &str,
    digest: SectionDigest,
) {
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    let state = context.host_mut().editor_state_mut();
    let Some(selected) = state.selected_node() else {
        state.editor_ui.section_panel.analytics_load_finished();
        return;
    };
    let mockups = mockup_fingerprint(selected);
    let link =
        op_editor_core::section::AnalyticsLink::new(key, name, digest, mockups, now_secs(), None);
    state.editor_ui.section_panel.attach_analytics(link);
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

/// The load stopped somewhere: the dialog was dismissed, the read failed, or
/// the store refused. The control comes back rather than staying inert.
fn finish_load<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    context
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .section_panel
        .analytics_load_finished();
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

/// The daemon base the reader was started with.
fn base_for<C: RepaintContext + 'static>(_inner: &Rc<RefCell<C>>) -> String {
    crate::daemon_base::daemon_base()
}

/// Seconds since the epoch, for a link's `linkedAt`.
fn now_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// Send the section the panel has been edited into, when it asked to be saved.
///
/// The widget layer has no HTTP, so the commit queues the properties and this
/// is what carries them: `POST /api/files/<key>/sections/<node>`. The route
/// compares what it is handed with what it holds, so the whole properties
/// object goes — a body carrying only the summary would read as "clear
/// everything else".
fn save_pending<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
    if SAVE_IN_FLIGHT.with(|slot| slot.get()) {
        return;
    }
    let Some(properties) = ({
        let Ok(mut context) = inner.try_borrow_mut() else {
            return;
        };
        let state = context.host_mut().editor_state_mut();
        state.editor_ui.section_panel.take_pending_save()
    }) else {
        return;
    };
    let (node, key) = {
        let Ok(context) = inner.try_borrow() else {
            return;
        };
        let state = context.host().editor_state();
        (
            state.editor_ui.section_panel.node.clone(),
            state.editor_ui.file_key.clone(),
        )
    };
    let (Some(node), Some(key)) = (node, key) else {
        return;
    };
    let Ok(body) = serde_json::to_string(&properties) else {
        fail_save(inner);
        return;
    };
    let url = crate::daemon_base::with_tenant_param(&format!(
        "{base}{}",
        section_routes::section(&key, node.as_str())
    ));
    SAVE_IN_FLIGHT.with(|slot| slot.set(true));
    let reply_inner = inner.clone();
    let started = live_sync::post_json_with_status(
        &url,
        &body,
        Rc::new(move |status, _body| {
            SAVE_IN_FLIGHT.with(|slot| slot.set(false));
            if status >= 400 {
                fail_save(&reply_inner);
                return;
            }
            let Ok(mut context) = reply_inner.try_borrow_mut() else {
                return;
            };
            let state = context.host_mut().editor_state_mut();
            state.editor_ui.section_panel.saved(properties.clone());
            context.host_mut().mark_editor_state_dirty();
            let _ = context.repaint();
        }),
    );
    if !started {
        SAVE_IN_FLIGHT.with(|slot| slot.set(false));
        fail_save(inner);
    }
}

/// The write did not land. The draft stays, so the retry is a keystroke.
fn fail_save<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    context
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .section_panel
        .save_refused();
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

/// Resolve one outstanding link, or hand the finished answer to the panel.
fn resolve_next<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
    let waiting = PENDING.with(|slot| {
        let pending = slot.borrow();
        let pending = pending.as_ref()?;
        pending
            .resolved
            .iter()
            .find(|(_, resolution)| matches!(resolution, Resolution::Waiting))
            .map(|(key, _)| key.clone())
    });
    let Some(key) = waiting else {
        deliver(inner);
        return;
    };
    // Marked before the request leaves, so the next tick cannot ask twice.
    // Marked as "nothing established" rather than as "no digest": a request
    // that never leaves (no XHR, a refused URL) is reported by the same arm,
    // and a deletion nobody confirmed must not be shown for it (#145).
    PENDING.with(|slot| {
        if let Some(pending) = slot.borrow_mut().as_mut() {
            for (entry_key, resolution) in pending.resolved.iter_mut() {
                if entry_key == &key {
                    *resolution = Resolution::Failed;
                }
            }
        }
    });
    let url = crate::daemon_base::with_tenant_param(&format!(
        "{base}{}",
        section_routes::analytics(&key)
    ));
    let reply_inner = inner.clone();
    let reply_key = key.clone();
    let _ = live_sync::get_with_status(
        &url,
        Rc::new(move |status, body| {
            // Read by the shared rule: a refusal is a refusal, a 404 is the
            // only confirmation of a deletion, and everything else says nothing
            // about the document (see [`Resolution::Failed`]).
            let resolution = answer::resolution(status, &body);
            PENDING.with(|slot| {
                if let Some(pending) = slot.borrow_mut().as_mut() {
                    for (entry_key, entry_resolution) in pending.resolved.iter_mut() {
                        if entry_key == &reply_key {
                            *entry_resolution = resolution.clone();
                        }
                    }
                }
            });
            resolve_next(&reply_inner, &crate::daemon_base::daemon_base());
        }),
    );
}

/// Hand the whole answer to the panel.
fn deliver<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let Some(pending) = PENDING.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    let links: Vec<SectionLink> = pending
        .properties
        .analytics
        .iter()
        .map(|link| {
            let answer = pending
                .resolved
                .iter()
                .find(|(key, _)| key == &link.key)
                .map(|(_, resolution)| resolution);
            // Called only when nothing is still waiting. `Answered(None)` is the
            // store saying it does not have the document — the only arm that
            // means GONE. A refusal and a check that did not complete each have
            // their own sentence, and the fall-through — a link nobody asked
            // about — claims nothing rather than accusing the asset, which
            // `link_state(.., None, ..)` would have done by calling it gone.
            let state = match answer {
                Some(Resolution::Refused) => refused_link_state(Some(link)),
                Some(Resolution::Failed) => unchecked_link_state(Some(link)),
                Some(Resolution::Answered(digest)) => {
                    link_state(Some(link), digest.as_ref(), &pending.mockups)
                }
                _ => unchecked_link_state(Some(link)),
            };
            SectionLink {
                link: link.clone(),
                state,
            }
        })
        .collect();
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    context
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .section_panel
        .apply(&pending.node, pending.properties, links);
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

/// The read failed: say so rather than showing an empty section.
fn fail<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, node: &NodeId) {
    let Ok(mut context) = inner.try_borrow_mut() else {
        return;
    };
    context
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .section_panel
        .fail(node);
    context.host_mut().mark_editor_state_dirty();
    let _ = context.repaint();
}

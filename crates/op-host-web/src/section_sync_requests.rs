//! The request and response legs of the section reader.
//!
//! Every read and write the panel asks for, and what each answer means: the
//! section's properties (`GET`), an attached analytics document's digest
//! (`GET`), loading a picked markdown file into the store (`POST`) and
//! attaching what it answers, and the properties write (`POST`). The state
//! machine that decides WHEN each one runs, and the pending answer they fill
//! in, live in the spine.

use std::cell::RefCell;
use std::rc::Rc;

use super::answer::{self, Resolution};
use super::{Pending, IN_FLIGHT, PENDING, SAVE_IN_FLIGHT};
use op_editor_core::editor_ui_state::section_panel::SectionLink;
use op_editor_core::section::{
    link_state, mockup_fingerprint, refused_link_state, unchecked_link_state, SectionDigest,
    SectionProperties,
};
use op_editor_core::{section_routes, NodeId};

use crate::dom_io::{open_file_picker, read_file, ReadMode};
use crate::live_sync;
use crate::repaint_ctx::RepaintContext;

/// Ask the daemon what this section carries.
pub(super) fn start_properties<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    key: &str,
    node: NodeId,
) {
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

/// The panel asked for an analytics document: open the file dialog, read what
/// the person picks, load it into the store, and attach what comes back.
///
/// The order is forced by where the facts live. The store issues the KEY and
/// knows the DIGEST of the markdown it just took; the document in this tab has
/// the section's own screens, which is the other half of the link. Neither side
/// can build the link alone, and this function is the one place both are in
/// hand.
pub(super) fn attach_requested<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    base: &str,
) -> bool {
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
pub(super) fn save_pending<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
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
pub(super) fn resolve_next<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>, base: &str) {
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

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

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::editor_ui_state::section_panel::SectionLink;
use op_editor_core::section::{link_state, mockup_fingerprint, SectionDigest, SectionProperties};
use op_editor_core::{section, section_routes, NodeId};

use crate::live_sync;
use crate::repaint_ctx::RepaintContext;

/// Tick cadence: fast enough that selecting a section feels answered, slow
/// enough that an idle editor costs nothing.
const TICK_MS: i32 = 250;

/// How far one attached analytics document has got.
enum Resolution {
    /// Not asked yet.
    Waiting,
    /// Asked, and this is what the store said — `None` meaning it does not have
    /// the document at all.
    Answered(Option<SectionDigest>),
}

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
}

/// Wire the section reader onto the mounted shell. Called from mount.
pub(crate) fn start<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let base = crate::daemon_base::daemon_base();
    let inner = inner.clone();
    let tick: Rc<dyn Fn()> = Rc::new(move || {
        watch_selection(&inner);
        resolve_next(&inner, &base);
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
    PENDING.with(|slot| {
        if let Some(pending) = slot.borrow_mut().as_mut() {
            for (entry_key, resolution) in pending.resolved.iter_mut() {
                if entry_key == &key {
                    *resolution = Resolution::Answered(None);
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
            let digest = (status < 400)
                .then(|| serde_json::from_str::<serde_json::Value>(&body).ok())
                .flatten()
                .and_then(|value| value.get("digest")?.as_str().map(SectionDigest::of_hex));
            PENDING.with(|slot| {
                if let Some(pending) = slot.borrow_mut().as_mut() {
                    for (entry_key, resolution) in pending.resolved.iter_mut() {
                        if entry_key == &reply_key {
                            *resolution = Resolution::Answered(digest.clone());
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
            let resolved = pending
                .resolved
                .iter()
                .find(|(key, _)| key == &link.key)
                .and_then(|(_, resolution)| match resolution {
                    Resolution::Answered(digest) => digest.clone(),
                    Resolution::Waiting => None,
                });
            SectionLink {
                link: link.clone(),
                state: link_state(Some(link), resolved.as_ref(), &pending.mockups),
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

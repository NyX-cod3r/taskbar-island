//! Source B: UserNotificationListener (spec §3).
//!
//! The listener sees toast notifications at the notification level, so it
//! works even when UIA never gets a handle to the toast window (Source A).
//! Registration requires the sparse package declared in §3; without it the
//! listener reports `Denied` and this source says so instead of pretending.
//!
//! Everything here is event driven: `NotificationChanged` is the only reason
//! a notification is ever read, and the thread then parks on the channel.

use std::sync::mpsc::{self, Receiver, Sender};

use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{
    NotificationKinds, UserNotification, UserNotificationChangedEventArgs,
    UserNotificationChangedKind,
};

use super::toast::{classify_toast, ToastCall};
use super::uiakit::{self, DumpNode};
use super::{CallSource, Emit, Signal, SignalKind, SourceCtx, SourceId};
use crate::activities::call_state::mask_number;

/// Package family name Phone Link ships under. The listener hands us the
/// publisher of every notification, so this gate replaces Source A's app
/// name match (spec §3 Source B).
const PHONE_PFN_PREFIX: &str = "Microsoft.YourPhone";

/// Whether a package family name belongs to Phone Link.
fn is_phone_pfn(pfn: &str) -> bool {
    pfn.starts_with(PHONE_PFN_PREFIX)
}

pub struct NotifySource;

impl CallSource for NotifySource {
    fn id(&self) -> SourceId {
        SourceId::Notify
    }

    fn label(&self) -> &'static str {
        "B: UserNotificationListener"
    }

    fn run(self: Box<Self>, ctx: SourceCtx, emit: Emit) {
        run(ctx, emit);
    }
}

/// What the listener told us about one notification id.
enum Event {
    Added(u32),
    Removed(u32),
}

fn run(ctx: SourceCtx, emit: Emit) {
    uiakit::init_mta();
    let diag = ctx.diag.clone();
    let note = |line: String| match &diag {
        Some(diag) => diag.log(&line),
        None => crate::log::log(&line),
    };

    let listener = match UserNotificationListener::Current() {
        Ok(listener) => listener,
        Err(error) => {
            note(format!("source B: listener unavailable: {error}"));
            return;
        }
    };
    // Consent is requested only when the system has not decided yet; with
    // the sparse package registered this is already `Allowed`.
    let mut status = listener
        .GetAccessStatus()
        .unwrap_or(UserNotificationListenerAccessStatus::Unspecified);
    if status == UserNotificationListenerAccessStatus::Unspecified {
        // The one place a consent prompt can appear — on this worker, never
        // on the UI thread.
        status = listener
            .RequestAccessAsync()
            .and_then(|operation| operation.join())
            .unwrap_or(UserNotificationListenerAccessStatus::Denied);
    }
    if status != UserNotificationListenerAccessStatus::Allowed {
        note(format!(
            "source B: notification access is {:?} (register the sparse package to grant it)",
            status
        ));
        return;
    }

    let (tx, rx): (Sender<Event>, Receiver<Event>) = mpsc::channel();
    let handler: TypedEventHandler<UserNotificationListener, UserNotificationChangedEventArgs> =
        TypedEventHandler::new(
            move |_sender: windows::core::Ref<'_, UserNotificationListener>,
                  args: windows::core::Ref<'_, UserNotificationChangedEventArgs>| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let kind = args
                    .ChangeKind()
                    .unwrap_or(UserNotificationChangedKind::Removed);
                let id = args.UserNotificationId().unwrap_or(0);
                let event = if kind == UserNotificationChangedKind::Added {
                    Event::Added(id)
                } else {
                    Event::Removed(id)
                };
                let _ = tx.send(event);
                Ok(())
            },
        );
    let token = match listener.NotificationChanged(&handler) {
        Ok(token) => token,
        Err(error) => {
            note(format!(
                "source B: NotificationChanged hook failed: {error}"
            ));
            return;
        }
    };

    // A ring already in progress when the island starts: adopt it. The
    // history also holds hours-old "Missed call" entries, so only live
    // states are taken from this first pass.
    let mut tracked: Option<u32> = None;
    if let Ok(history) = listener
        .GetNotificationsAsync(NotificationKinds::Toast)
        .and_then(|operation| operation.join())
    {
        let count = history.Size().unwrap_or(0);
        for index in 0..count {
            let Ok(item) = history.GetAt(index) else {
                continue;
            };
            // Startup pass: only live states are adopted — the history also
            // holds hours-old "Missed call" entries.
            match classify_notification(&ctx, &item) {
                Some(ToastCall::Incoming { caller }) => {
                    note(format!(
                        "source B: history shows an incoming call caller={}",
                        preview(caller.as_deref())
                    ));
                    tracked = Some(item.Id().unwrap_or(0));
                    emit.send(Signal::new(SourceId::Notify, SignalKind::Incoming, caller));
                }
                Some(ToastCall::Dialing { caller }) => {
                    note(format!(
                        "source B: history shows a call being placed caller={}",
                        preview(caller.as_deref())
                    ));
                    tracked = Some(item.Id().unwrap_or(0));
                    emit.send(Signal::new(SourceId::Notify, SignalKind::Dialing, caller));
                }
                Some(ToastCall::Ended { .. }) => {}
                None => {}
            }
        }
    }
    note("source B ready: UserNotificationListener on NotificationChanged".into());

    while let Ok(event) = rx.recv() {
        match event {
            Event::Added(id) => {
                let Ok(item) = listener.GetNotification(id) else {
                    continue;
                };
                match classify_notification(&ctx, &item) {
                    Some(ToastCall::Incoming { caller }) => {
                        note(format!(
                            "source B: notification {id} incoming caller={}",
                            preview(caller.as_deref())
                        ));
                        tracked = Some(id);
                        emit.send(Signal::new(SourceId::Notify, SignalKind::Incoming, caller));
                    }
                    Some(ToastCall::Dialing { caller }) => {
                        note(format!(
                            "source B: notification {id} dialing caller={}",
                            preview(caller.as_deref())
                        ));
                        tracked = Some(id);
                        emit.send(Signal::new(SourceId::Notify, SignalKind::Dialing, caller));
                    }
                    Some(ToastCall::Ended { caller }) => {
                        // A missed/declined-call entry can close a ring this
                        // source is holding; it never starts one.
                        note(format!(
                            "source B: notification {id} ended caller={}",
                            preview(caller.as_deref())
                        ));
                        emit.send(Signal::new(SourceId::Notify, SignalKind::Ended, caller));
                    }
                    None => {}
                }
            }
            Event::Removed(id) => {
                if tracked == Some(id) {
                    tracked = None;
                    note(format!("source B: notification {id} removed -> Ended"));
                    emit.send(Signal::new(SourceId::Notify, SignalKind::Ended, None));
                }
            }
        }
    }
    let _ = listener.RemoveNotificationChanged(token);
}

/// PFN gate + text extraction + classification for one notification.
fn classify_notification(ctx: &SourceCtx, user: &UserNotification) -> Option<ToastCall> {
    let app = user.AppInfo().ok()?;
    let pfn = app.PackageFamilyName().ok()?.to_string();
    if !is_phone_pfn(&pfn) {
        if let Some(diag) = &ctx.diag {
            diag.log(&format!("source B: skipping notification from {pfn}"));
        }
        return None;
    }
    let nodes = notification_texts(user);
    if nodes.is_empty() {
        if let Some(diag) = &ctx.diag {
            diag.log("source B: Phone Link notification carried no text elements");
        }
        return None;
    }
    if let Some(diag) = &ctx.diag {
        let joined = nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        diag.log(&format!("source B: texts {}", mask_number(&joined)));
    }
    // The publisher is already Phone Link, so the classifier's app gate is
    // opened: `&[]` means "no name to match against".
    classify_toast(&nodes, &[])
}

/// Every `<text>` of every binding, as classification input.
fn notification_texts(user: &UserNotification) -> Vec<DumpNode> {
    let mut nodes = Vec::new();
    let Ok(notification) = user.Notification() else {
        return nodes;
    };
    let Ok(visual) = notification.Visual() else {
        return nodes;
    };
    let Ok(bindings) = visual.Bindings() else {
        return nodes;
    };
    let binding_count = bindings.Size().unwrap_or(0);
    for binding_index in 0..binding_count {
        let Ok(binding) = bindings.GetAt(binding_index) else {
            continue;
        };
        let Ok(texts) = binding.GetTextElements() else {
            continue;
        };
        let text_count = texts.Size().unwrap_or(0);
        for text_index in 0..text_count {
            let Ok(text) = texts.GetAt(text_index) else {
                continue;
            };
            let Ok(value) = text.Text() else {
                continue;
            };
            let value = value.to_string();
            if !value.trim().is_empty() {
                nodes.push(DumpNode {
                    ctrl: "Text".into(),
                    name: value,
                });
            }
        }
    }
    nodes
}

/// Caller text for log lines only: long digit runs are masked.
fn preview(caller: Option<&str>) -> String {
    caller.map(mask_number).unwrap_or_else(|| "-".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(list: &[&str]) -> Vec<DumpNode> {
        list.iter()
            .map(|name| DumpNode {
                ctrl: "Text".into(),
                name: (*name).to_string(),
            })
            .collect()
    }

    #[test]
    fn only_the_phone_link_package_passes_the_gate() {
        assert!(is_phone_pfn("Microsoft.YourPhone_8wekyb3d8bbwe"));
        assert!(!is_phone_pfn(
            "Microsoft.Windows.ShellExperienceHost_cw5n1h2txyewy"
        ));
        assert!(!is_phone_pfn(""));
        assert!(!is_phone_pfn("YourPhone_8wekyb3d8bbwe"));
    }

    #[test]
    fn an_incoming_text_only_toast_classifies_without_buttons() {
        let nodes = texts(&["Incoming call", "Priya Sharma", "Mobile"]);
        assert_eq!(
            classify_toast(&nodes, &[]),
            Some(ToastCall::Incoming {
                caller: Some("Priya Sharma".into())
            })
        );
    }

    #[test]
    fn a_missed_call_text_reports_ended() {
        let nodes = texts(&["Missed call", "Bo Chan", "2 minutes ago"]);
        assert_eq!(
            classify_toast(&nodes, &[]),
            Some(ToastCall::Ended {
                caller: Some("Bo Chan".into())
            })
        );
    }

    #[test]
    fn a_plain_message_notification_is_not_a_call() {
        let nodes = texts(&["New message", "Ada Lovelace", "See you at five"]);
        assert_eq!(classify_toast(&nodes, &[]), None);
    }
}

use gpui::{prelude::*, px, rgb, Context};

use concord::discord::PresenceStatus;
use concord::t;

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{avatar, column, row};
use crate::ui::composer::Composer;
use crate::ui::emoji;
use crate::ui::overlay;
use crate::ui::stream;
use crate::ui::switcher;
use crate::ui::workspace::{
    activity_kind_label, audit_line, emoji_summary, invite_summary, ActivityDraft, AttachmentViewerZoom, Prompt, ServerTab, Workspace,
};

impl Workspace {
    pub(super) fn overlays_impl(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let entity = cx.entity();

        if self.self_profile_popout {
            let user_name = self
                .last_state
                .as_ref()
                .and_then(|s| s.current_user())
                .unwrap_or("blu")
                .to_string();

            let dismiss = {
                let entity = entity.clone();
                move |cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.self_profile_popout = false;
                        cx.notify();
                    });
                }
            };

            let presence_entity = entity.clone();
            let set_presence = move |status: PresenceStatus, cx: &mut gpui::App| {
                presence_entity.update(cx, |workspace, cx| {
                    workspace.status = status;
                    workspace.self_profile_popout = false;
                    workspace.set_status(status);
                    cx.notify();
                });
            };

            let edit_status_fn = {
                let entity = entity.clone();
                move |cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.self_profile_popout = false;
                        let mut text = Composer::default();
                        text.set_text(&workspace.custom_status);
                        workspace.editing_status = Some(text);
                        cx.notify();
                    });
                }
            };

            let open_settings_fn = {
                let entity = entity.clone();
                move |cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.self_profile_popout = false;
                        workspace.open_settings_window(cx);
                    });
                }
            };

            let current_status = self.status;

            let card = gpui::div()
                .absolute()
                .bottom(px(80.))
                .left(px(72.))
                .w(px(280.))
                .bg(rgb(active().surface_sunken))
                .border_1()
                .border_color(rgb(active().border))
                .rounded(px(layout::RADIUS_LG))
                .shadow_lg()
                .overflow_hidden()
                .child(
                    column()
                        .w_full()
                        .child(
                            gpui::div()
                                .w_full()
                                .h(px(60.))
                                .bg(rgb(active().accent)),
                        )
                        .child(
                            column()
                                .p(px(space::MD))
                                .gap(px(space::SM))
                                .child(
                                    row()
                                        .items_center()
                                        .gap(px(space::SM))
                                        .child(avatar(48., &user_name))
                                        .child(
                                            column()
                                                .child(
                                                    gpui::div()
                                                        .text_size(px(scaled(text::BASE)))
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(rgb(active().text))
                                                        .child(user_name.clone()),
                                                )
                                                .child(
                                                    gpui::div()
                                                        .text_size(px(scaled(text::XS)))
                                                        .text_color(rgb(active().text_subtle))
                                                        .child(format!("@{user_name}")),
                                                ),
                                        ),
                                )
                                .child(
                                    gpui::div()
                                        .w_full()
                                        .h(px(1.))
                                        .bg(rgb(active().border)),
                                )
                                .child(
                                    column()
                                        .gap(px(2.))
                                        .child(Self::self_status_item("Online", PresenceStatus::Online, current_status, {
                                            let sp = set_presence.clone();
                                            move |cx| sp(PresenceStatus::Online, cx)
                                        }))
                                        .child(Self::self_status_item("Idle", PresenceStatus::Idle, current_status, {
                                            let sp = set_presence.clone();
                                            move |cx| sp(PresenceStatus::Idle, cx)
                                        }))
                                        .child(Self::self_status_item("Do Not Disturb", PresenceStatus::DoNotDisturb, current_status, {
                                            let sp = set_presence.clone();
                                            move |cx| sp(PresenceStatus::DoNotDisturb, cx)
                                        }))
                                        .child(Self::self_status_item("Offline", PresenceStatus::Offline, current_status, {
                                            let sp = set_presence.clone();
                                            move |cx| sp(PresenceStatus::Offline, cx)
                                        })),
                                )
                                .child(
                                    gpui::div()
                                        .w_full()
                                        .h(px(1.))
                                        .bg(rgb(active().border)),
                                )
                                .child(
                                    row()
                                        .id("popout-edit-status")
                                        .w_full()
                                        .px(px(space::SM))
                                        .py(px(space::XS))
                                        .rounded(px(layout::RADIUS))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(active().surface_hover)))
                                        .on_click(move |_, _, cx| edit_status_fn(cx))
                                        .child(
                                            gpui::div()
                                                .text_size(px(scaled(text::SM)))
                                                .text_color(rgb(active().text))
                                                .child("Set Custom Status"),
                                        ),
                                )
                                .child(
                                    row()
                                        .id("popout-edit-profile")
                                        .w_full()
                                        .px(px(space::SM))
                                        .py(px(space::XS))
                                        .rounded(px(layout::RADIUS))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(active().surface_hover)))
                                        .on_click(move |_, _, cx| open_settings_fn(cx))
                                        .child(
                                            gpui::div()
                                                .text_size(px(scaled(text::SM)))
                                                .text_color(rgb(active().text))
                                                .child("Edit Profile"),
                                        ),
                                ),
                        ),
                );

            return Some(
                gpui::div()
                    .absolute()
                    .inset_0()
                    .child(
                        gpui::div()
                            .id("self-profile-dismiss")
                            .absolute()
                            .inset_0()
                            .on_click(move |_event, _window, cx| dismiss(cx)),
                    )
                    .child(card),
            );
        }

        if let Some(pending) = &self.confirming {
            let prompt = pending.action.prompt();
            return Some(overlay::scrim().child(overlay::confirm_view(
                prompt,
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.confirm();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.confirming = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some((action, dont_ask)) = &self.risk {
            let dont_ask = *dont_ask;
            return Some(overlay::scrim().child(overlay::risk_warning_view(
                overlay::RiskWarning {
                    title: &t!("warning-title"),
                    body: &action.body(),
                    dont_ask_label: &t!("warning-dont-ask-again"),
                    dont_ask,
                    continue_label: &t!("warning-continue"),
                    cancel_label: &t!("action-cancel"),
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            if let Some((_, dont_ask)) = &mut workspace.risk {
                                *dont_ask = !*dont_ask;
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.accept_risk();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.risk = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(view) = &self.viewing_image
            && let Some(url) = view.url()
            && let Some(image) = self.attachment_previews.get(url)
        {
            let (max_width, max_height) = match view.zoom {
                AttachmentViewerZoom::Default => (720., 540.),
                AttachmentViewerZoom::Large => (1080., 810.),
                AttachmentViewerZoom::Fullscreen => (1920., 1440.),
            };
            let position =
                (view.urls.len() > 1).then(|| format!("{} / {}", view.index + 1, view.urls.len()));

            let close = {
                let entity = entity.clone();
                move |cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.viewing_image = None;
                        cx.notify();
                    });
                }
            };

            return Some(
                gpui::div().child(
                    overlay::scrim()
                        .id("image-viewer-scrim")
                        .on_click(move |_event, _window, cx| close(cx))
                        .child(overlay::image_viewer_view(
                            gpui::ImageSource::Image(image.clone()),
                            position,
                            max_width,
                            max_height,
                            {
                                let entity = entity.clone();
                                move |forward, cx: &mut gpui::App| {
                                    entity.update(cx, |workspace, cx| {
                                        workspace.step_viewed_image(forward);
                                        cx.notify();
                                    });
                                }
                            },
                            {
                                let entity = entity.clone();
                                move |in_, cx: &mut gpui::App| {
                                    entity.update(cx, |workspace, cx| {
                                        workspace.zoom_viewed_image(in_);
                                        cx.notify();
                                    });
                                }
                            },
                        )),
                ),
            );
        }

        if let Some(menu) = &self.context_menu {
            let items = self.context_items(menu.subject);
            let at = menu.at;

            let dismiss = {
                let entity = entity.clone();
                move |cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.context_menu = None;
                        cx.notify();
                    });
                }
            };

            return Some(
                gpui::div().child(
                    gpui::div()
                        .id("context-dismiss")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .on_click(move |_event, _window, cx| dismiss(cx))
                        .child(overlay::context_menu_view(&items, at, {
                            let entity = entity.clone();
                            move |index, cx: &mut gpui::App| {
                                entity.update(cx, |workspace, cx| {
                                    workspace.pick_context_item(index);
                                    cx.notify();
                                });
                            }
                        })),
                ),
            );
        }

        if let Some(channel_id) = self.deleting_channel {
            let name = self.channel_name(channel_id);
            return Some(overlay::scrim().child(overlay::confirm_view(
                &format!(
                    "{} #{name}? {}",
                    t!("action-delete-channel"),
                    t!("warning-delete-channel")
                ),
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.deleting_channel = None;
                            workspace.delete_channel_confirmed(channel_id);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.deleting_channel = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(view) = &self.permission_grid {
            let rows: Vec<overlay::PermissionRow> = concord::discord::permissions_catalogue::ALL
                .iter()
                .map(|permission| overlay::PermissionRow {
                    label: permission.label.to_owned(),
                    description: permission.description.to_owned(),
                    setting: if permission.is_set(view.allow) {
                        overlay::PermissionState::Allow
                    } else if permission.is_set(view.deny) {
                        overlay::PermissionState::Deny
                    } else {
                        overlay::PermissionState::Inherit
                    },
                })
                .collect();

            return Some(overlay::scrim().child(overlay::permission_grid_view(
                &view.name,
                &rows,
                view.is_dirty(),
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.cycle_permission(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.save_permissions();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.permission_grid = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(view) = &self.account {
            let fields: Vec<(String, String, String, bool)> = concord::discord::AccountField::ALL
                .into_iter()
                .enumerate()
                .map(|(index, field)| {
                    (
                        field.label().to_owned(),
                        view.form.display_value(field),
                        field.hint().to_owned(),
                        index == view.focused,
                    )
                })
                .collect();
            let problem = view.form.problem().map(|p| p.message());
            let uri = view
                .totp_secret
                .as_ref()
                .map(|secret| secret.otpauth_uri(self.current_user_name().unwrap_or("Discord")));
            let codes: Vec<(String, bool)> = view
                .backup_codes
                .iter()
                .map(|code| (code.code.clone(), code.consumed))
                .collect();

            return Some(overlay::scrim().child(overlay::account_view(
                overlay::AccountPanel {
                    fields: &fields,
                    problem: problem.as_deref(),
                    enrolment_uri: uri.as_deref(),
                    enrolment_code: &view.totp_code,
                    backup_codes: &codes,
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.focus_account_field(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |key: &str, cx: &mut gpui::App| {
                        let key = key.to_owned();
                        entity.update(cx, |workspace, cx| {
                            workspace.type_account_key(&key);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |key: &str, cx: &mut gpui::App| {
                        let key = key.to_owned();
                        entity.update(cx, |workspace, cx| {
                            workspace.type_totp_code(&key);
                            cx.notify();
                        });
                    }
                },
                overlay::AccountActions {
                    save: {
                        let entity = entity.clone();
                        Box::new(move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.submit_account_form();
                                cx.notify();
                            });
                        })
                    },
                    enrol: {
                        let entity = entity.clone();
                        Box::new(move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.toggle_totp_enrolment();
                                cx.notify();
                            });
                        })
                    },
                    submit_enrolment: {
                        let entity = entity.clone();
                        Box::new(move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.submit_totp_enrolment();
                                cx.notify();
                            });
                        })
                    },
                    disable: {
                        let entity = entity.clone();
                        Box::new(move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.disable_totp();
                                cx.notify();
                            });
                        })
                    },
                    backup_codes: {
                        let entity = entity.clone();
                        Box::new(move |regenerate: bool, cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.load_backup_codes(regenerate);
                                cx.notify();
                            });
                        })
                    },
                    close: {
                        let entity = entity.clone();
                        Box::new(move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.account = None;
                                cx.notify();
                            });
                        })
                    },
                },
            )));
        }

        if let Some(view) = &self.access {
            let sessions = view.sessions.iter().map(|session| overlay::AccessRow {
                primary: if session.platform.is_empty() {
                    t!("label-session")
                } else {
                    session.platform.clone()
                },
                secondary: session.summary(),
                action: if view.logout_targets.contains(&session.id_hash) {
                    t!("action-deselect")
                } else {
                    t!("action-select-for-logout")
                },
                destructive: false,
                selected: view.logout_targets.contains(&session.id_hash),
            });
            let apps = view.apps.iter().map(|app| overlay::AccessRow {
                primary: app.name.clone(),
                secondary: app.summary(),
                action: t!("action-revoke"),
                destructive: true,
                selected: false,
            });
            let rows: Vec<overlay::AccessRow> = sessions.chain(apps).collect();
            let selected_any = !view.logout_targets.is_empty();
            let masked = view.masked_password();

            return Some(overlay::scrim().child(overlay::access_view(
                overlay::AccessPanel {
                    rows: &rows,
                    loading: view.loading,
                    error: view.error.as_deref(),
                    password: selected_any.then_some(masked.as_str()),
                    logout_enabled: selected_any && !view.password.is_empty(),
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.activate_access_row(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |key: &str, cx: &mut gpui::App| {
                        let key = key.to_owned();
                        entity.update(cx, |workspace, cx| {
                            workspace.type_access_password(&key);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.log_out_selected_sessions();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.access = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if self.privacy_open {
            let state = self.privacy_state();
            let rows: Vec<overlay::PrivacyRow> = concord::discord::PrivacySetting::ALL
                .into_iter()
                .map(|setting| overlay::PrivacyRow::new(setting, &state))
                .collect();

            return Some(overlay::scrim().child(overlay::privacy_view(
                &rows,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.toggle_privacy_setting(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.privacy_open = false;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(view) = &self.connections {
            let rows: Vec<overlay::ConnectionRow> = view
                .connections
                .iter()
                .map(overlay::ConnectionRow::new)
                .collect();

            return Some(overlay::scrim().child(overlay::connections_view(
                &rows,
                view.loading,
                view.error.as_deref(),
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.toggle_connection_visibility(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.toggle_connection_activity(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.unlink_connection(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.connections = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(view) = &self.soundboard {
            let rows: Vec<overlay::SoundRow> = view
                .sounds()
                .map(|sound| overlay::SoundRow {
                    label: sound.label().to_owned(),
                    name: sound.name.clone(),
                    available: sound.available,
                })
                .collect();

            return Some(overlay::scrim().child(overlay::soundboard_view(
                &rows,
                view.loading,
                view.error.as_deref(),
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.play_sound(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.soundboard = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(view) = &self.server_management {
            let tabs: Vec<(String, bool)> = ServerTab::ALL
                .iter()
                .map(|tab| (tab.label(), *tab == view.tab))
                .collect();

            let membership = self.membership_rows();
            let members = self.server_member_rows();
            let onboarding = self.onboarding_rows();
            let discovery = self.discovery_rows();
            let (rows, empty_label) = match view.tab {
                ServerTab::Events => (
                    view.events
                        .iter()
                        .map(|event| overlay::ServerRow {
                            primary: event.name.clone(),
                            secondary: Some(event.summary()),
                            action: Some(if event.status.is_cancellable() {
                                t!("action-cancel-event")
                            } else {
                                t!("action-delete")
                            }),
                            secondary_action: Some(t!("action-edit")),
                            tertiary_action: Some(t!("action-interested")),
                        })
                        .collect::<Vec<_>>(),
                    t!("status-no-events"),
                ),
                ServerTab::Stickers => (
                    view.stickers
                        .iter()
                        .map(|sticker| overlay::ServerRow {
                            primary: sticker.name.clone(),
                            secondary: Some(sticker.summary()),
                            action: Some(t!("action-delete")),
                            secondary_action: Some(t!("action-rename")),
                            tertiary_action: None,
                        })
                        .collect::<Vec<_>>(),
                    t!("status-no-stickers"),
                ),
                ServerTab::Discovery => (
                    discovery
                        .iter()
                        .map(|(label, value)| overlay::ServerRow {
                            primary: label.clone(),
                            secondary: Some(value.clone()),
                            action: None,
                            secondary_action: Some(t!("action-change")),
                            tertiary_action: None,
                        })
                        .collect::<Vec<_>>(),
                    t!("status-loading"),
                ),
                ServerTab::Onboarding => (
                    onboarding
                        .iter()
                        .map(|row| match row {
                            concord::discord::OnboardingRow::Question {
                                title,
                                summary,
                                unanswered,
                            } => overlay::ServerRow {
                                primary: title.clone(),
                                secondary: Some(if *unanswered {
                                    format!("{summary} - {}", t!("status-needs-an-answer"))
                                } else {
                                    summary.clone()
                                }),
                                action: None,
                                secondary_action: None,
                                tertiary_action: None,
                            },
                            concord::discord::OnboardingRow::Option {
                                title,
                                summary,
                                picked,
                                ..
                            } => overlay::ServerRow {
                                primary: format!(
                                    "   {} {title}",
                                    if *picked { "\u{25CF}" } else { "\u{25CB}" }
                                ),
                                secondary: Some(summary.clone()),
                                action: None,
                                secondary_action: Some(if *picked {
                                    t!("action-unpick")
                                } else {
                                    t!("action-pick")
                                }),
                                tertiary_action: None,
                            },
                        })
                        .collect::<Vec<_>>(),
                    t!("status-no-onboarding"),
                ),
                ServerTab::Members => (
                    members
                        .iter()
                        .map(|member| overlay::ServerRow {
                            primary: member.name.clone(),
                            secondary: Some(member.summary()),
                            action: Some(t!("action-ban")),
                            secondary_action: Some(t!("action-view-profile")),
                            tertiary_action: None,
                        })
                        .collect::<Vec<_>>(),
                    t!("status-no-members-loaded"),
                ),
                ServerTab::Templates => (
                    view.templates
                        .iter()
                        .map(|template| overlay::ServerRow {
                            primary: template.name.clone(),
                            secondary: Some(format!("{} - {}", template.url(), template.summary())),
                            action: Some(t!("action-delete")),
                            secondary_action: Some(t!("action-sync")),
                            tertiary_action: None,
                        })
                        .collect::<Vec<_>>(),
                    t!("status-no-templates"),
                ),
                ServerTab::Membership => (
                    membership
                        .iter()
                        .map(|(label, value)| overlay::ServerRow {
                            primary: label.clone(),
                            secondary: Some(value.clone()),
                            action: None,
                            secondary_action: Some(t!("action-change")),
                            tertiary_action: None,
                        })
                        .collect::<Vec<_>>(),
                    t!("status-loading"),
                ),
                ServerTab::Settings => (
                    view.settings
                        .iter()
                        .map(|(label, value)| overlay::ServerRow {
                            primary: label.clone(),
                            secondary: Some(value.clone()),
                            action: None,
                            secondary_action: (label == &t!("label-name"))
                                .then(|| t!("action-rename")),
                            tertiary_action: None,
                        })
                        .collect::<Vec<_>>(),
                    t!("status-loading"),
                ),
                ServerTab::Roles => (
                    view.roles
                        .iter()
                        .map(|role| overlay::ServerRow {
                            primary: role.name.clone(),
                            secondary: Some(concord::i18n::translate_text(
                                "status-role-permissions",
                                &[(
                                    "count",
                                    &concord::discord::permissions_catalogue::ALL
                                        .iter()
                                        .filter(|permission| permission.is_set(role.permissions))
                                        .count()
                                        .to_string(),
                                )],
                            )),
                            action: Some(t!("action-delete")),
                            secondary_action: Some(t!("action-permissions")),
                            tertiary_action: Some(t!("action-move-up")),
                        })
                        .collect(),
                    t!("status-no-roles"),
                ),
                ServerTab::Sounds => (
                    view.sounds
                        .iter()
                        .map(|sound| overlay::ServerRow {
                            primary: sound.label().to_owned(),
                            secondary: (!sound.available).then(|| t!("status-unavailable")),
                            action: Some(t!("action-delete")),
                            secondary_action: Some(t!("action-rename")),
                            tertiary_action: None,
                        })
                        .collect(),
                    t!("status-no-sounds"),
                ),
                ServerTab::AutoMod => (
                    view.automod
                        .iter()
                        .map(|rule| overlay::ServerRow {
                            primary: format!(
                                "{} {}",
                                if rule.enabled { "\u{25CF}" } else { "\u{25CB}" },
                                rule.name
                            ),
                            secondary: Some(rule.summary()),
                            action: Some(t!("action-delete")),
                            secondary_action: Some(if rule.enabled {
                                t!("action-disable")
                            } else {
                                t!("action-enable")
                            }),
                            tertiary_action: None,
                        })
                        .collect(),
                    t!("status-no-automod"),
                ),
                ServerTab::Invites => (
                    view.invites
                        .iter()
                        .map(|invite| overlay::ServerRow {
                            primary: format!("discord.gg/{}", invite.code),
                            secondary: Some(invite_summary(invite)),
                            action: Some(t!("action-revoke")),
                            secondary_action: None,
                            tertiary_action: None,
                        })
                        .collect::<Vec<_>>(),
                    t!("status-no-invites"),
                ),
                ServerTab::Emoji => (
                    view.emojis
                        .iter()
                        .map(|emoji| overlay::ServerRow {
                            primary: format!(":{}:", emoji.name),
                            secondary: emoji_summary(emoji),
                            action: Some(t!("action-delete")),
                            secondary_action: Some(t!("action-rename")),
                            tertiary_action: None,
                        })
                        .collect(),
                    t!("status-no-emoji"),
                ),
                ServerTab::AuditLog => (
                    view.audit_log
                        .iter()
                        .map(|entry| overlay::ServerRow {
                            primary: audit_line(entry),
                            secondary: entry.reason.clone(),
                            action: None,
                            secondary_action: None,
                            tertiary_action: None,
                        })
                        .collect(),
                    t!("status-no-audit-entries"),
                ),
            };

            let add_emoji_label = t!("action-add-emoji");
            let add_role_label = t!("action-new-role");
            let server_settings_label = t!("action-server-settings");
            let add_template_label = t!("action-new-template");
            let add_event_label = t!("action-new-event");
            let finish_onboarding_label = t!("action-finish-onboarding");
            let add_sticker_label = t!("action-add-sticker");
            return Some(overlay::scrim().child(overlay::server_management_view(
                overlay::ServerPanel {
                    tabs: &tabs,
                    rows: &rows,
                    empty_label: &empty_label,
                    loading: view.loading,
                    error: view.error.as_deref(),
                    add_label: match view.tab {
                        ServerTab::Emoji => Some(add_emoji_label.as_str()),
                        ServerTab::Roles => Some(add_role_label.as_str()),
                        ServerTab::Settings => Some(server_settings_label.as_str()),
                        ServerTab::Templates => Some(add_template_label.as_str()),
                        ServerTab::Events => Some(add_event_label.as_str()),
                        ServerTab::Onboarding => Some(finish_onboarding_label.as_str()),
                        ServerTab::Stickers => Some(add_sticker_label.as_str()),
                        _ => None,
                    },
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            if let Some(tab) = ServerTab::ALL.get(index) {
                                workspace.select_server_tab(*tab);
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    let tab = view.tab;
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            match tab {
                                ServerTab::Invites => workspace.revoke_invite(index),
                                ServerTab::Emoji => workspace.delete_emoji(index),
                                ServerTab::Roles => workspace.delete_role(index),
                                ServerTab::Sounds => workspace.delete_sound(index),
                                ServerTab::AutoMod => workspace.delete_automod_rule(index),
                                ServerTab::Events => workspace.remove_event(index),
                                ServerTab::Members => workspace.ban_listed_member(index),
                                ServerTab::Stickers => workspace.delete_sticker(index),
                                ServerTab::Templates => workspace.delete_template(index),
                                ServerTab::AuditLog
                                | ServerTab::Settings
                                | ServerTab::Membership
                                | ServerTab::Onboarding
                                | ServerTab::Discovery => {}
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    let tab = view.tab;
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            match tab {
                                ServerTab::Emoji => workspace.start_emoji_rename(index),
                                ServerTab::Sounds => workspace.start_sound_rename(index),
                                ServerTab::Roles => workspace.open_role_permissions(index),
                                ServerTab::AutoMod => workspace.toggle_automod_rule(index),
                                ServerTab::Membership => workspace.activate_membership_row(index),
                                ServerTab::Events => workspace.start_event_edit(index),
                                ServerTab::Members => workspace.open_listed_member(index),
                                ServerTab::Stickers => workspace.start_sticker_rename(index),
                                ServerTab::Onboarding => workspace.pick_onboarding_answer(index),
                                ServerTab::Discovery => workspace.activate_discovery_row(index),
                                ServerTab::Templates => workspace.sync_template(index),
                                ServerTab::Settings if index == 0 => {
                                    let mut text = Composer::default();
                                    if let Some(view) = &workspace.server_management
                                        && let Some((_, value)) = view.settings.first()
                                    {
                                        text.set_text(value);
                                    }
                                    workspace.prompt = Some((Prompt::GuildName, text));
                                }
                                _ => {}
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    let tab = view.tab;
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            match tab {
                                ServerTab::Events => workspace.mark_event_interest(index),
                                ServerTab::Roles => workspace.move_role(index, true),
                                _ => {}
                            }
                            cx.notify();
                        });
                    }
                },
                overlay::ServerPanelActions {
                    reload: Box::new({
                        let entity = entity.clone();
                        move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.reload_server_tab();
                                cx.notify();
                            });
                        }
                    }),
                    add: Box::new({
                        let entity = entity.clone();
                        let tab = view.tab;
                        move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.prompt = Some(match tab {
                                    ServerTab::Roles => (Prompt::NewRole, Composer::default()),
                                    ServerTab::Templates => {
                                        (Prompt::NewTemplate, Composer::default())
                                    }
                                    ServerTab::Onboarding => {
                                        workspace.submit_onboarding();
                                        cx.notify();
                                        return;
                                    }
                                    ServerTab::Events => (Prompt::NewEvent, Composer::default()),
                                    ServerTab::Stickers => {
                                        (Prompt::NewSticker, Composer::default())
                                    }
                                    ServerTab::Settings => (Prompt::GuildIcon, Composer::default()),
                                    _ => (Prompt::EmojiImage, Composer::default()),
                                });
                                cx.notify();
                            });
                        }
                    }),
                    close: Box::new({
                        let entity = entity.clone();
                        move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                workspace.server_management = None;
                                cx.notify();
                            });
                        }
                    }),
                },
            )));
        }

        if let Some(view) = &self.bans {
            let rows: Vec<_> = view
                .bans
                .iter()
                .map(|ban| overlay::BanRow {
                    username: ban.username.clone(),
                    reason: ban.reason.clone(),
                })
                .collect();

            let status = view.error.clone().or_else(|| {
                if view.loading {
                    Some("Loading bans...".to_string())
                } else if view.bans.is_empty() {
                    Some("Nobody is banned from this server".to_string())
                } else {
                    None
                }
            });

            return Some(overlay::scrim().child(overlay::ban_list_view(
                &rows,
                status.as_deref(),
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.unban(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.prompt = Some((Prompt::BulkBan, Composer::default()));
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.bans = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if self.editing_roles.is_some() {
            let roles = self.guild_roles();

            return Some(overlay::scrim().child(overlay::role_picker_view(
                &roles,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.toggle_role(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.save_roles();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.editing_roles = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if self.sticker_picker {
            let choices: Vec<_> = self
                .guild_stickers()
                .into_iter()
                .map(|(name, url)| overlay::StickerChoice {
                    name,
                    image: url.and_then(|url| self.attachment_previews.get(&url).cloned()),
                })
                .collect();

            return Some(overlay::scrim().child(overlay::sticker_picker_view(
                &choices,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.stage_sticker(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.sticker_picker = false;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(invite) = &self.invite {
            let row = overlay::InviteRow {
                guild_name: invite
                    .preview
                    .as_ref()
                    .map(|preview| preview.guild_name.clone())
                    .unwrap_or_else(|| "Looking up invite...".to_string()),
                channel_name: invite
                    .preview
                    .as_ref()
                    .and_then(|preview| preview.channel_name.clone()),
                inviter: invite
                    .preview
                    .as_ref()
                    .and_then(|preview| preview.inviter.clone()),
                member_count: invite.preview.as_ref().and_then(|p| p.member_count),
                online_count: invite.preview.as_ref().and_then(|p| p.online_count),
                already_joined: invite
                    .preview
                    .as_ref()
                    .is_some_and(|preview| preview.already_joined),
                status: invite
                    .error
                    .clone()
                    .or_else(|| invite.preview.is_none().then(|| "Resolving...".to_string())),
            };

            return Some(overlay::scrim().child(overlay::invite_view(
                &row,
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.accept_invite();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.invite = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some((prompt, text)) = &self.prompt {
            let (title, placeholder) = (prompt.title(), prompt.placeholder());
            let discovery = matches!(prompt, Prompt::InviteCode).then(|| {
                let rows: Vec<overlay::DiscoveryRow> = self
                    .discovered
                    .iter()
                    .map(|guild| overlay::DiscoveryRow {
                        name: guild.name.clone(),
                        summary: guild.summary(),
                        joinable: guild.is_joinable(),
                    })
                    .collect();
                overlay::discovery_results(&rows, self.discovering, {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.join_discovered_guild(index);
                            cx.notify();
                        });
                    }
                })
            });

            return Some(overlay::scrim().child(overlay::text_prompt_view(
                title,
                placeholder,
                text.text(),
                discovery,
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.submit_prompt();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.prompt = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(draft) = &self.editing_activity {
            let kinds: Vec<(String, bool)> = ActivityDraft::KINDS
                .iter()
                .map(|kind| (activity_kind_label(*kind), *kind == draft.kind))
                .collect();
            let labels = [
                ("label-activity-name", "hint-activity-name"),
                ("label-activity-details", "hint-activity-details"),
                ("label-activity-state", "hint-activity-state"),
            ];
            let fields: Vec<overlay::ActivityField> = labels
                .iter()
                .enumerate()
                .map(|(index, (label, hint))| overlay::ActivityField {
                    label: t!(*label),
                    placeholder: t!(*hint),
                    value: draft.fields[index].text().to_owned(),
                    focused: draft.focused == index,
                })
                .collect();

            return Some(overlay::scrim().child(overlay::activity_editor_view(
                &kinds,
                &fields,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            if let Some(draft) = &mut workspace.editing_activity
                                && let Some(kind) = ActivityDraft::KINDS.get(index)
                            {
                                draft.kind = *kind;
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            if let Some(draft) = &mut workspace.editing_activity {
                                draft.focused = index.min(ActivityDraft::FIELDS - 1);
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.submit_activity();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.clear_activity();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.editing_activity = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(text) = &self.editing_status {
            return Some(overlay::scrim().child(overlay::text_prompt_view(
                "Custom status",
                "What are you up to?",
                text.text(),
                None,
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.submit_custom_status();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.editing_status = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some((_, name)) = &self.renaming_folder {
            return Some(overlay::scrim().child(overlay::text_prompt_view(
                "Rename folder",
                "Type a name",
                name.text(),
                None,
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.submit_folder_rename();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.renaming_folder = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(switcher) = &self.switcher {
            return Some(
                overlay::scrim()
                    .items_start()
                    .pt(px(96.))
                    .child(switcher::switcher_view(switcher)),
            );
        }

        if let Some(picker) = &self.picker {
            let cursor = picker.cursor;
            return Some(overlay::scrim().child(emoji::picker_view(cursor, {
                let entity = entity.clone();
                move |glyph: &'static str, cx: &mut gpui::App| {
                    entity.update(cx, |workspace, cx| {
                        workspace.pick_emoji(glyph);
                        cx.notify();
                    });
                }
            })));
        }

        if let Some(picker) = &self.stream_picker {
            return Some(overlay::scrim().child(stream::picker_view(
                picker,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.start_stream(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.stream_picker = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some(devices) = &self.audio_devices {
            return Some(overlay::scrim().child(overlay::audio_devices_view(
                &devices.inputs,
                &devices.outputs,
                devices.selected_input.as_deref(),
                devices.selected_output.as_deref(),
                devices.error.as_deref(),
                {
                    let entity = entity.clone();
                    move |is_input, id, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            if is_input {
                                workspace.set_audio_device(Some(id.clone()), None);
                            } else {
                                workspace.set_audio_device(None, Some(id.clone()));
                            }
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.audio_devices = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        if let Some((_, glyph, users)) = &self.reaction_users {
            return Some(
                overlay::scrim().child(overlay::reaction_users_view(glyph, users, {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.reaction_users = None;
                            cx.notify();
                        });
                    }
                })),
            );
        }

        if let Some(mentions) = &self.inbox {
            let rows: Vec<_> = mentions
                .iter()
                .map(|mention| overlay::InboxRow {
                    author: mention.author.clone(),
                    content: mention.content.clone(),
                })
                .collect();

            return Some(overlay::scrim().child(overlay::inbox_view(
                &rows,
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.open_mention(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.dismiss_mention(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = entity.clone();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.inbox = None;
                            cx.notify();
                        });
                    }
                },
            )));
        }

        None
    }
}

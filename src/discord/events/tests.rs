use super::*;
use crate::discord::{AttachmentInfo, AttachmentMediaType};

#[cfg(test)]
fn poll_result_info_from_fields<'a>(
    fields: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<crate::discord::PollInfo> {
    use crate::discord::PollAnswerInfo;
    let mut question = None;
    let mut winner_id = None;
    let mut winner_text = None;
    let mut winner_votes = None;
    let mut total_votes = None;
    for (name, value) in fields {
        match name {
            "poll_question_text" => question = Some(value.to_owned()),
            "victor_answer_id" => winner_id = value.parse::<u8>().ok(),
            "victor_answer_text" => winner_text = Some(value.to_owned()),
            "victor_answer_votes" => winner_votes = value.parse::<u64>().ok(),
            "total_votes" => total_votes = value.parse::<u64>().ok(),
            _ => {}
        }
    }

    let question = question.unwrap_or_else(|| "Poll results".to_owned());
    let answers = winner_text
        .map(|text| {
            vec![PollAnswerInfo {
                answer_id: winner_id.unwrap_or(1),
                text,
                vote_count: winner_votes,
                me_voted: false,
            }]
        })
        .unwrap_or_default();

    Some(crate::discord::PollInfo {
        answers,
        results_finalized: Some(true),
        total_votes,
        ..crate::discord::PollInfo::test(question)
    })
}

#[cfg(test)]
mod cases {
    use super::*;

    #[test]
    pub fn attachment_media_classification_controls_inline_preview() {
        let video = attachment_info("clip.mp4", Some("video/mp4"));
        assert!(video.media_type() == Some(AttachmentMediaType::Video));
        assert_eq!(video.inline_preview_url(), None);
        assert_eq!(
            video.inline_preview_info().map(|info| (
                info.url,
                info.proxy_url,
                info.proxy_preview_only,
            )),
            Some((
                "https://media.discordapp.net/clip.mp4",
                Some("https://media.discordapp.net/clip.mp4"),
                true,
            ))
        );

        let image = attachment_info("cat.png", Some("image/png"));
        assert!(image.media_type() == Some(AttachmentMediaType::Image));
        assert_eq!(
            image.inline_preview_url(),
            Some("https://cdn.discordapp.com/cat.png")
        );
        assert_eq!(
            image.inline_preview_info().and_then(|info| info.proxy_url),
            Some("https://media.discordapp.net/cat.png")
        );

        assert!(attachment_info("CAT.PNG", None).media_type() == Some(AttachmentMediaType::Image));
        assert!(attachment_info("CLIP.MP4", None).media_type() == Some(AttachmentMediaType::Video));
        assert!(
            attachment_info("MUSIC.MP3", None).media_type() == Some(AttachmentMediaType::Audio)
        );
    }

    #[test]
    pub fn poll_result_embed_fields_map_to_poll_summary() {
        let poll = poll_result_info_from_fields([
            ("poll_question_text", "오늘 뭐 먹지?"),
            ("victor_answer_id", "1"),
            ("victor_answer_text", "김치찌개"),
            ("victor_answer_votes", "5"),
            ("total_votes", "7"),
        ])
        .expect("poll result fields should map");

        assert_eq!(poll.question, "오늘 뭐 먹지?");
        assert_eq!(poll.total_votes, Some(7));
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].text, "김치찌개");
        assert_eq!(poll.answers[0].vote_count, Some(5));
    }

    #[test]
    pub fn event_metadata_routes_each_delivery_category() {
        use crate::discord::ids::Id;
        use crate::discord::{PremiumTier, SnapshotAreas};
        let cases = [
            (
                "mutating, snapshot only",
                AppEvent::MessageDeleteBulk {
                    guild_id: Some(Id::new(1)),
                    channel_id: Id::new(10),
                    message_ids: vec![Id::new(20), Id::new(30)],
                },
                Some(SnapshotAreas::message()),
                false,
            ),
            (
                "mutating, also delivered as an effect",
                AppEvent::CurrentUserCapabilities {
                    premium_tier: PremiumTier::Nitro,
                },
                Some(SnapshotAreas::navigation()),
                true,
            ),
            ("effect only", AppEvent::GatewayClosed, None, true),
            (
                "inert",
                AppEvent::GuildUnavailable {
                    guild_id: Id::new(1),
                },
                None,
                false,
            ),
            (
                "typing updates shared member and message identity",
                AppEvent::TypingStart {
                    guild_id: Some(Id::new(1)),
                    channel_id: Id::new(10),
                    user_id: Id::new(20),
                    member: None,
                },
                Some(SnapshotAreas::navigation_and_message()),
                false,
            ),
            (
                "ready user directory joins guild and message identity",
                AppEvent::ReadyUserDirectory {
                    users: vec![crate::discord::ChannelRecipientInfo::test(
                        Id::new(20),
                        "Ready User",
                    )],
                },
                Some(SnapshotAreas::navigation_and_message()),
                false,
            ),
        ];

        for (label, event, expected_areas, expected_effect) in cases {
            assert_eq!(event.snapshot_areas(), expected_areas, "{label}");
            assert_eq!(event.needs_effect_delivery(), expected_effect, "{label}");
        }
    }

    pub fn attachment_info(filename: &str, content_type: Option<&str>) -> AttachmentInfo {
        use crate::discord::ids::Id;
        AttachmentInfo {
            url: format!("https://cdn.discordapp.com/{filename}"),
            proxy_url: format!("https://media.discordapp.net/{filename}"),
            content_type: content_type.map(str::to_owned),
            size: 1024,
            width: Some(640),
            height: Some(480),
            ..AttachmentInfo::test(Id::new(1), filename)
        }
    }
}

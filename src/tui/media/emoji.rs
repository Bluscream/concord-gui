use std::{collections::HashSet, time::Instant};

use ratatui_image::{picker::Picker, protocol::Protocol};

use crate::{
    config::ImageProtocolPreference,
    discord::{AppCommand, AppEvent},
    tui::ui::EmojiImage,
};

use super::{
    EmojiImageTarget,
    cache::{MediaImageCacheCore, MediaImageCacheEntry},
    decode::{DecodedMediaImage, MediaImageDecodeJob, MediaImageDecodeKey},
    emoji_protocol, query_image_picker,
};

/// Cap on the URL-keyed emoji image cache. Animated entries retain decoded
/// frames, so the cache must stay bounded even though Discord emoji files are
/// small on the wire.
pub(super) const MAX_EMOJI_IMAGE_CACHE_ENTRIES: usize = 128;

pub(in crate::tui) struct EmojiImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) cache: MediaImageCacheCore<String, EmojiImageEntry>,
}

pub(super) enum EmojiImageEntry {
    Loading {
        last_used: u64,
    },
    Decoding {
        generation: u64,
        last_used: u64,
    },
    Ready {
        image: DecodedMediaImage,
        protocol: Protocol,
        protocol_frame_index: usize,
        last_used: u64,
    },
    Failed {
        last_used: u64,
    },
}

impl MediaImageCacheEntry for EmojiImageEntry {
    fn last_used(&self) -> u64 {
        match self {
            EmojiImageEntry::Loading { last_used }
            | EmojiImageEntry::Decoding { last_used, .. }
            | EmojiImageEntry::Ready { last_used, .. }
            | EmojiImageEntry::Failed { last_used } => *last_used,
        }
    }

    fn touch(&mut self, tick: u64) {
        match self {
            EmojiImageEntry::Loading { last_used }
            | EmojiImageEntry::Decoding { last_used, .. }
            | EmojiImageEntry::Ready { last_used, .. }
            | EmojiImageEntry::Failed { last_used } => *last_used = tick,
        }
    }

    fn is_loading(&self) -> bool {
        matches!(self, EmojiImageEntry::Loading { .. })
    }

    fn decoding_generation(&self) -> Option<u64> {
        match self {
            EmojiImageEntry::Decoding { generation, .. } => Some(*generation),
            EmojiImageEntry::Loading { .. }
            | EmojiImageEntry::Ready { .. }
            | EmojiImageEntry::Failed { .. } => None,
        }
    }
}

impl EmojiImageCache {
    #[cfg(test)]
    pub(in crate::tui) fn new() -> Self {
        Self::new_with_protocol_preference(ImageProtocolPreference::Auto)
    }

    pub(in crate::tui) fn new_with_protocol_preference(
        protocol_preference: ImageProtocolPreference,
    ) -> Self {
        Self {
            picker: query_image_picker(
                "emoji",
                "emoji image picker unavailable",
                protocol_preference,
            ),
            cache: MediaImageCacheCore::new(),
        }
    }

    /// Returns decoded protocols for visible targets and refreshes their
    /// LRU timestamps so they survive the next pruning pass.
    pub(in crate::tui) fn render_state(
        &mut self,
        targets: &[EmojiImageTarget],
    ) -> Vec<EmojiImage<'_>> {
        for target in targets {
            let touch_tick = self.cache.next_tick();
            if let Some(entry) = self.cache.entries.get_mut(&target.url) {
                entry.touch(touch_tick);
                if let EmojiImageEntry::Ready {
                    image,
                    protocol,
                    protocol_frame_index,
                    ..
                } = entry
                    && *protocol_frame_index != image.current_frame_index()
                    && let Some(picker) = self.picker.as_ref()
                    && let Some(updated_protocol) = emoji_protocol(picker, image.current_frame())
                {
                    *protocol = updated_protocol;
                    *protocol_frame_index = image.current_frame_index();
                }
            }
        }
        targets
            .iter()
            .filter_map(|target| {
                let EmojiImageEntry::Ready { protocol, .. } =
                    self.cache.entries.get(&target.url)?
                else {
                    return None;
                };
                Some(EmojiImage {
                    url: target.url.clone(),
                    protocol,
                })
            })
            .collect()
    }

    pub(in crate::tui) fn next_requests(
        &mut self,
        targets: &[EmojiImageTarget],
    ) -> Vec<AppCommand> {
        if self.picker.is_none() {
            return Vec::new();
        }

        let mut intents = Vec::new();
        for target in targets.iter().take(MAX_EMOJI_IMAGE_CACHE_ENTRIES) {
            if self
                .cache
                .insert_loading(target.url.clone(), |last_used| EmojiImageEntry::Loading {
                    last_used,
                })
            {
                intents.push(AppCommand::LoadAttachmentPreview {
                    url: target.url.clone(),
                });
            }
        }
        self.prune_to_limit(targets);
        intents
    }

    pub(in crate::tui) fn record_event(&mut self, event: &AppEvent) -> Option<MediaImageDecodeJob> {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, .. } => {
                self.store_failed(url);
                None
            }
            _ => None,
        }
    }

    /// Drops LRU entries while protecting URLs in the current frame's
    /// targets so a flood of unique ids can never evict what is on screen.
    pub(super) fn prune_to_limit(&mut self, targets: &[EmojiImageTarget]) {
        let protected: HashSet<&str> = targets
            .iter()
            .take(MAX_EMOJI_IMAGE_CACHE_ENTRIES)
            .map(|target| target.url.as_str())
            .collect();
        self.cache
            .prune_to_limit(MAX_EMOJI_IMAGE_CACHE_ENTRIES, |url| {
                protected.contains(url.as_str())
            });
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) -> Option<MediaImageDecodeJob> {
        self.cache.start_decode_job(
            url.to_owned(),
            std::sync::Arc::from(bytes.to_vec()),
            self.picker.is_some(),
            |generation, last_used| EmojiImageEntry::Decoding {
                generation,
                last_used,
            },
            |last_used| EmojiImageEntry::Failed { last_used },
            MediaImageDecodeKey::Emoji,
        )
    }

    pub(in crate::tui) fn store_decoded(
        &mut self,
        url: String,
        result_generation: u64,
        result: std::result::Result<DecodedMediaImage, String>,
    ) {
        if !self
            .cache
            .decoded_generation_matches(&url, result_generation)
        {
            return;
        }

        let last_used = self.cache.next_tick();
        match result {
            Ok(image) => {
                let Some(picker) = self.picker.as_ref() else {
                    self.cache
                        .entries
                        .insert(url, EmojiImageEntry::Failed { last_used });
                    return;
                };
                let Some(protocol) = emoji_protocol(picker, image.current_frame()) else {
                    self.cache
                        .entries
                        .insert(url, EmojiImageEntry::Failed { last_used });
                    return;
                };
                self.cache.entries.insert(
                    url,
                    EmojiImageEntry::Ready {
                        image,
                        protocol,
                        protocol_frame_index: 0,
                        last_used,
                    },
                );
            }
            Err(_) => {
                self.cache
                    .entries
                    .insert(url, EmojiImageEntry::Failed { last_used });
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        self.cache
            .store_failed_if_present(url.to_owned(), |last_used| EmojiImageEntry::Failed {
                last_used,
            });
    }

    pub(in crate::tui) fn sync_animation_visibility(
        &mut self,
        targets: &[EmojiImageTarget],
        now: Instant,
    ) {
        let visible = targets
            .iter()
            .map(|target| target.url.as_str())
            .collect::<HashSet<_>>();
        for (url, entry) in &mut self.cache.entries {
            let EmojiImageEntry::Ready { image, .. } = entry else {
                continue;
            };
            if visible.contains(url.as_str()) {
                image.start_animation(now);
            } else {
                image.pause_animation();
            }
        }
    }

    pub(in crate::tui) fn pause_animations(&mut self) {
        for entry in self.cache.entries.values_mut() {
            if let EmojiImageEntry::Ready { image, .. } = entry {
                image.pause_animation();
            }
        }
    }

    pub(in crate::tui) fn next_animation_deadline(&self) -> Option<Instant> {
        self.cache
            .entries
            .values()
            .filter_map(|entry| match entry {
                EmojiImageEntry::Ready { image, .. } => image.next_frame_deadline(),
                _ => None,
            })
            .min()
    }

    pub(in crate::tui) fn advance_animations(&mut self, now: Instant) -> bool {
        let mut advanced = false;
        for entry in self.cache.entries.values_mut() {
            if let EmojiImageEntry::Ready { image, .. } = entry {
                advanced |= image.advance_frame(now);
            }
        }
        advanced
    }
}

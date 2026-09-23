use super::payloads::*;
use super::*;
use std::collections::{HashMap, VecDeque};
use tokio::time::Instant;

impl Default for GuildMemberRequestScheduler {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            in_flight: None,
            awaiting_response: VecDeque::new(),
            guild_rate_limit_until: HashMap::new(),
            next_nonce: 1,
        }
    }
}

impl GuildMemberRequest {
    pub(super) fn payload(&self) -> String {
        match &self.kind {
            GuildMemberRequestKind::Search {
                query,
                limit,
                presences,
            } => {
                search_guild_members_payload(self.guild_id, query, *limit, *presences, &self.nonce)
            }
            GuildMemberRequestKind::ByIds {
                user_ids,
                presences,
            } => request_guild_members_by_ids_payload(
                self.guild_id,
                user_ids,
                *presences,
                &self.nonce,
            ),
        }
    }

    pub(super) fn is_search(&self) -> bool {
        matches!(&self.kind, GuildMemberRequestKind::Search { .. })
    }
}

impl GuildMemberRequestScheduler {
    pub(super) fn enqueue_search(
        &mut self,
        guild_id: Id<GuildMarker>,
        query: String,
        limit: u16,
        presences: bool,
        nonce: String,
        now: Instant,
    ) -> bool {
        self.prune_expired(now);
        let request = GuildMemberRequest {
            guild_id,
            nonce,
            kind: GuildMemberRequestKind::Search {
                query,
                limit,
                presences,
            },
        };
        self.awaiting_response
            .retain(|sent| sent.request.nonce != request.nonce);

        if let Some(pending) = self
            .pending
            .iter_mut()
            .rev()
            .find(|pending| pending.request.guild_id == guild_id && pending.request.is_search())
        {
            pending.request = request;
            return true;
        }

        if self.pending.len() >= MAX_PENDING_GUILD_MEMBER_REQUESTS {
            let Some(position) = self
                .pending
                .iter()
                .position(|pending| pending.request.is_search())
            else {
                return false;
            };
            self.pending.remove(position);
        }

        self.enqueue_request(request, now)
    }

    pub(super) fn enqueue_by_ids(
        &mut self,
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
        presences: bool,
        now: Instant,
    ) {
        self.prune_expired(now);
        let mut seen = BTreeSet::new();
        let mut remaining = user_ids
            .into_iter()
            .filter(|user_id| seen.insert(*user_id))
            .collect::<Vec<_>>();

        let compatible_requests = self.pending.iter().filter(|pending| {
            pending.request.guild_id == guild_id
                && matches!(
                    pending.request.kind,
                    GuildMemberRequestKind::ByIds {
                        presences: pending_presences,
                        ..
                    } if pending_presences == presences
                )
        });
        let mut available = 0usize;
        for pending in compatible_requests {
            let GuildMemberRequestKind::ByIds { user_ids, .. } = &pending.request.kind else {
                continue;
            };
            remaining.retain(|user_id| !user_ids.contains(user_id));
            available += 100usize.saturating_sub(user_ids.len());
        }
        let new_request_count = remaining.len().saturating_sub(available).div_ceil(100);
        self.make_room_for_by_ids(new_request_count);

        for pending in self.pending.iter_mut().filter(|pending| {
            pending.request.guild_id == guild_id
                && matches!(
                    pending.request.kind,
                    GuildMemberRequestKind::ByIds {
                        presences: pending_presences,
                        ..
                    } if pending_presences == presences
                )
        }) {
            let GuildMemberRequestKind::ByIds { user_ids, .. } = &mut pending.request.kind else {
                continue;
            };
            remaining.retain(|user_id| !user_ids.contains(user_id));
            let available = 100usize.saturating_sub(user_ids.len());
            let added = remaining.len().min(available);
            user_ids.extend(remaining.drain(..added));
            if remaining.is_empty() {
                return;
            }
        }

        for user_ids in remaining.chunks(100) {
            let request = GuildMemberRequest {
                guild_id,
                nonce: self.next_nonce(),
                kind: GuildMemberRequestKind::ByIds {
                    user_ids: user_ids.to_vec(),
                    presences,
                },
            };
            self.enqueue_by_ids_request(request, now);
        }
    }

    pub(super) fn make_room_for_by_ids(&mut self, additional: usize) {
        let overflow = self
            .pending
            .len()
            .saturating_add(additional)
            .saturating_sub(MAX_PENDING_GUILD_MEMBER_REQUESTS);
        if overflow == 0 {
            return;
        }
        let positions = self
            .pending
            .iter()
            .enumerate()
            .filter_map(|(index, pending)| pending.request.is_search().then_some(index))
            .take(overflow)
            .collect::<Vec<_>>();
        for position in positions.into_iter().rev() {
            self.pending.remove(position);
        }
    }

    pub(super) fn enqueue_request(&mut self, request: GuildMemberRequest, now: Instant) -> bool {
        if self.pending.len() >= MAX_PENDING_GUILD_MEMBER_REQUESTS {
            return false;
        }
        let send_at = self.guild_request_at(request.guild_id, now);
        self.pending
            .push_back(PendingGuildMemberRequest { request, send_at });
        true
    }

    pub(super) fn enqueue_by_ids_request(&mut self, request: GuildMemberRequest, now: Instant) {
        // Hydration requests resolve concrete users already visible in the UI,
        // so losing them is worse than briefly exceeding the search queue's
        // soft memory bound. IDs are deduplicated and grouped into 100-member
        // payloads here. The shared Gateway writer enforces the connection-wide
        // send budget, while RATE_LIMITED dispatches provide any request-specific
        // delay that Discord requires.
        let send_at = self.guild_request_at(request.guild_id, now);
        self.pending
            .push_back(PendingGuildMemberRequest { request, send_at });
    }

    pub(super) fn next_nonce(&mut self) -> String {
        let nonce = format!("concord-{:016x}", self.next_nonce);
        self.next_nonce = self.next_nonce.wrapping_add(1).max(1);
        nonce
    }

    pub(super) fn next_delay(&self, now: Instant) -> Option<Duration> {
        self.pending
            .iter()
            .map(|request| request.send_at.saturating_duration_since(now))
            .min()
    }

    pub(super) fn start_due(&mut self, now: Instant) -> Option<String> {
        if self.in_flight.is_some() {
            return None;
        }
        let index = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, request)| request.send_at <= now)
            .min_by_key(|(_, request)| request.send_at)
            .map(|(index, _)| index)?;
        let pending = self
            .pending
            .remove(index)
            .expect("due guild member request exists");
        let payload = pending.request.payload();
        self.in_flight = Some(ScheduledGuildMemberRequest {
            request: pending.request,
            accepted: false,
            retry_at: None,
        });
        Some(payload)
    }

    pub(super) fn complete_send(&mut self, sent_at: Instant) {
        let completed = self
            .in_flight
            .take()
            .expect("sent guild member request exists");
        if let Some(retry_at) = completed.retry_at {
            self.pending.push_front(PendingGuildMemberRequest {
                request: completed.request,
                send_at: retry_at,
            });
            return;
        }

        if completed.accepted {
            return;
        }

        self.prune_expired(sent_at);
        if self.awaiting_response.len() >= MAX_SENT_GUILD_MEMBER_REQUESTS {
            self.awaiting_response.pop_front();
        }
        self.awaiting_response.push_back(SentGuildMemberRequest {
            request: completed.request,
            sent_at,
        });
    }

    pub(super) fn cancel_in_flight(&mut self, now: Instant) {
        let Some(in_flight) = self.in_flight.take() else {
            return;
        };
        if in_flight.accepted {
            return;
        }
        let send_at = in_flight.retry_at.unwrap_or(now);
        self.pending.push_front(PendingGuildMemberRequest {
            request: in_flight.request,
            send_at,
        });
    }

    pub(super) fn apply_rate_limit(
        &mut self,
        guild_id: Id<GuildMarker>,
        nonce: Option<&str>,
        retry_after: Duration,
        now: Instant,
    ) {
        self.prune_expired(now);
        let retry_at = now + retry_after;
        let has_newer_search = self
            .pending
            .iter()
            .any(|pending| pending.request.guild_id == guild_id && pending.request.is_search());

        let in_flight_matches = self.in_flight.as_ref().is_some_and(|in_flight| {
            in_flight.request.guild_id == guild_id
                && nonce.is_none_or(|nonce| in_flight.request.nonce == nonce)
        });
        if in_flight_matches {
            let in_flight = self
                .in_flight
                .as_mut()
                .expect("matching guild member request is in flight");
            if in_flight.request.is_search() && has_newer_search {
                in_flight.accepted = true;
                in_flight.retry_at = None;
            } else {
                in_flight.retry_at = Some(retry_at);
                in_flight.accepted = false;
            }
            self.apply_guild_rate_limit_until(guild_id, retry_at);
            return;
        }

        let position = match nonce {
            Some(nonce) => self
                .awaiting_response
                .iter()
                .rposition(|sent| sent.request.guild_id == guild_id && sent.request.nonce == nonce),
            None => self
                .awaiting_response
                .iter()
                .rposition(|sent| sent.request.guild_id == guild_id),
        };
        if let Some(position) = position {
            let sent = self
                .awaiting_response
                .remove(position)
                .expect("rate-limited guild member request exists");
            if !sent.request.is_search() || !has_newer_search {
                self.pending.push_front(PendingGuildMemberRequest {
                    request: sent.request,
                    send_at: retry_at,
                });
            }
        }
        self.apply_guild_rate_limit_until(guild_id, retry_at);
    }

    pub(super) fn acknowledge(&mut self, nonce: &str) {
        if let Some(in_flight) = self.in_flight.as_mut()
            && in_flight.request.nonce == nonce
        {
            in_flight.accepted = true;
            return;
        }
        if let Some(position) = self
            .awaiting_response
            .iter()
            .rposition(|sent| sent.request.nonce == nonce)
        {
            self.awaiting_response.remove(position);
            return;
        }
        if let Some(position) = self
            .pending
            .iter()
            .rposition(|pending| pending.request.nonce == nonce)
        {
            self.pending.remove(position);
        }
    }

    fn guild_request_at(&self, guild_id: Id<GuildMarker>, requested_at: Instant) -> Instant {
        self.guild_rate_limit_until
            .get(&guild_id)
            .copied()
            .unwrap_or(requested_at)
            .max(requested_at)
    }

    pub(super) fn apply_guild_rate_limit_until(
        &mut self,
        guild_id: Id<GuildMarker>,
        retry_at: Instant,
    ) {
        let earliest = *self
            .guild_rate_limit_until
            .entry(guild_id)
            .and_modify(|current| *current = (*current).max(retry_at))
            .or_insert(retry_at);
        self.delay_pending_guild_until(guild_id, earliest);
    }

    fn delay_pending_guild_until(&mut self, guild_id: Id<GuildMarker>, earliest: Instant) {
        for pending in self
            .pending
            .iter_mut()
            .filter(|pending| pending.request.guild_id == guild_id)
        {
            pending.send_at = pending.send_at.max(earliest);
        }
    }

    pub(super) fn prune_expired(&mut self, now: Instant) {
        while self.awaiting_response.front().is_some_and(|sent| {
            now.saturating_duration_since(sent.sent_at) >= GUILD_MEMBER_REQUEST_RESPONSE_TTL
        }) {
            self.awaiting_response.pop_front();
        }
        self.guild_rate_limit_until
            .retain(|_, retry_at| *retry_at > now);
    }

    pub(super) fn prepare_reconnect(&mut self, now: Instant) {
        self.prune_expired(now);
        self.cancel_in_flight(now);
        self.recover_awaiting(now);
    }

    pub(super) fn recover_awaiting(&mut self, earliest: Instant) {
        let mut recovered = VecDeque::new();
        let mut recovered_guilds = HashSet::new();
        while let Some(sent) = self.awaiting_response.pop_back() {
            let superseded_search = sent.request.is_search()
                && self.pending.iter().chain(recovered.iter()).any(|pending| {
                    pending.request.guild_id == sent.request.guild_id && pending.request.is_search()
                });
            if !superseded_search {
                recovered_guilds.insert(sent.request.guild_id);
                recovered.push_front(PendingGuildMemberRequest {
                    request: sent.request,
                    send_at: earliest,
                });
            }
        }
        recovered.append(&mut self.pending);
        self.pending = recovered;
        for guild_id in recovered_guilds {
            self.delay_pending_guild_until(guild_id, earliest);
        }
    }

    pub(super) fn start_new_session(&mut self, now: Instant) {
        self.prune_expired(now);
        self.cancel_in_flight(now);
        self.recover_awaiting(now);
        // A new session is a clean slate for pacing, but a RATE_LIMITED reply
        // is about the account, not the connection, so its deadline survives
        // the reidentify.
        for pending in &mut self.pending {
            let earliest = self
                .guild_rate_limit_until
                .get(&pending.request.guild_id)
                .copied()
                .unwrap_or(now)
                .max(now);
            pending.send_at = pending.send_at.max(earliest);
        }
    }
}

impl InFlightGuildMemberRequest {
    pub(super) async fn wait(&mut self) -> Result<(), String> {
        (&mut self.completion)
            .await
            .map_err(|_| "gateway writer task stopped before send completed".to_owned())?
    }
}

impl SubscriptionDeduper {
    pub(super) fn should_send(&mut self, command: &GatewayCommand) -> bool {
        match command {
            GatewayCommand::SubscribeDirectMessage { channel_id } => {
                self.direct_messages.insert(*channel_id)
            }
            GatewayCommand::SubscribeGuildChannel {
                guild_id: _,
                channel_id: _,
            }
            | GatewayCommand::UpdateMemberListSubscription { .. } => true,
            _ => true,
        }
    }
}

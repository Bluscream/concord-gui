//! Creating, editing, reordering and deleting roles.
//!
//! The permission bitfields here are the same ones
//! `crate::discord::permissions_catalogue` enumerates: this module moves them,
//! that one names them.

use serde_json::{Map, Value, json};

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker},
};

use super::DiscordRest;

/// Discord's cap on a role name.
pub const MAX_ROLE_NAME_CHARS: usize = 100;

/// What to change about a role.
///
/// `None` means "leave alone", for the same reason `ChannelEdit` works this
/// way: sending the whole role back would overwrite fields this client never
/// showed, and Discord's role object has several - icons, unicode emoji, tags -
/// that it does not yet.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoleEdit {
    pub name: Option<String>,
    /// `Some(None)` clears the colour back to default.
    pub color: Option<Option<u32>>,
    /// Whether members with this role are listed separately.
    pub hoist: Option<bool>,
    /// Whether anyone can @mention it.
    pub mentionable: Option<bool>,
    pub permissions: Option<u64>,
}

impl RoleEdit {
    /// Whether this would change anything.
    ///
    /// An empty edit is not sent: it would spend a request and write an audit
    /// log entry saying nothing happened.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.color.is_none()
            && self.hoist.is_none()
            && self.mentionable.is_none()
            && self.permissions.is_none()
    }

    fn to_body(&self) -> Value {
        let mut fields = Map::new();
        if let Some(name) = &self.name {
            fields.insert(
                "name".to_owned(),
                Value::from(name.chars().take(MAX_ROLE_NAME_CHARS).collect::<String>()),
            );
        }
        if let Some(color) = &self.color {
            // Discord's "no colour" is zero, not null: a role with colour 0
            // inherits, which is what clearing means here.
            fields.insert("color".to_owned(), Value::from(color.unwrap_or(0)));
        }
        if let Some(hoist) = self.hoist {
            fields.insert("hoist".to_owned(), Value::from(hoist));
        }
        if let Some(mentionable) = self.mentionable {
            fields.insert("mentionable".to_owned(), Value::from(mentionable));
        }
        if let Some(permissions) = self.permissions {
            // A string, which is Discord's own convention for this field. The
            // top permission today is bit 53, so the mask sits exactly on the
            // largest integer a JSON number represents exactly - one more bit
            // and a number would start losing them.
            fields.insert(
                "permissions".to_owned(),
                Value::from(permissions.to_string()),
            );
        }
        Value::Object(fields)
    }
}

impl DiscordRest {
    /// Create a role.
    ///
    /// Created with no permissions and no colour, like Discord's own "new
    /// role": granting anything at creation would be a guess, and the editor
    /// opens on it immediately anyway.
    pub async fn create_role(&self, guild_id: Id<GuildMarker>, name: &str) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/guilds/{}/roles",
                    guild_id.get()
                ))
                .json(&json!({
                    "name": name.chars().take(MAX_ROLE_NAME_CHARS).collect::<String>(),
                    "permissions": "0",
                })),
            "create role",
        )
        .await
    }

    pub async fn modify_role(
        &self,
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
        edit: &RoleEdit,
    ) -> Result<()> {
        if edit.is_empty() {
            return Ok(());
        }

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/roles/{}",
                    guild_id.get(),
                    role_id.get()
                ))
                .json(&edit.to_body()),
            "modify role",
        )
        .await
    }

    pub async fn delete_role(
        &self,
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/roles/{}",
                guild_id.get(),
                role_id.get()
            )),
            "delete role",
        )
        .await
    }

    /// Move roles within a guild.
    ///
    /// One request for all of them, as with channels: position is what decides
    /// which role wins a permission conflict, so the intermediate states of a
    /// sequence of moves would briefly hand out the wrong permissions.
    pub async fn reorder_roles(
        &self,
        guild_id: Id<GuildMarker>,
        positions: &[(Id<RoleMarker>, u32)],
    ) -> Result<()> {
        let body: Vec<Value> = positions
            .iter()
            .map(|(role_id, position)| {
                json!({ "id": role_id.get().to_string(), "position": position })
            })
            .collect();

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/roles",
                    guild_id.get()
                ))
                .json(&body),
            "reorder roles",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::permissions_catalogue;

    #[test]
    fn an_empty_edit_is_not_sent() {
        assert!(RoleEdit::default().is_empty());
        assert!(
            !RoleEdit {
                hoist: Some(true),
                ..RoleEdit::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn permissions_are_sent_as_a_string() {
        // Discord's own convention for this field, and there is no headroom
        // left to lose: the highest permission today is bit 53, exactly the
        // largest integer a JSON number holds exactly.
        let every_permission = permissions_catalogue::ALL
            .iter()
            .fold(0u64, |mask, permission| mask | permission.mask());
        let edit = RoleEdit {
            permissions: Some(every_permission),
            ..RoleEdit::default()
        };

        assert_eq!(
            edit.to_body()["permissions"],
            Value::from(every_permission.to_string())
        );
        assert!(
            every_permission > (1u64 << 53),
            "the whole set must exceed what a JSON number holds, or this proves nothing"
        );
    }

    #[test]
    fn clearing_a_colour_sends_zero_rather_than_null() {
        // Discord's "no colour" is 0; null is rejected.
        let cleared = RoleEdit {
            color: Some(None),
            ..RoleEdit::default()
        };
        assert_eq!(cleared.to_body()["color"], Value::from(0));

        let untouched = RoleEdit {
            name: Some("mods".to_owned()),
            ..RoleEdit::default()
        };
        assert!(untouched.to_body().get("color").is_none());
    }

    #[test]
    fn a_name_is_truncated_on_character_boundaries() {
        let edit = RoleEdit {
            name: Some("é".repeat(200)),
            ..RoleEdit::default()
        };

        let name = edit.to_body()["name"].as_str().unwrap().to_owned();
        assert_eq!(name.chars().count(), MAX_ROLE_NAME_CHARS);
    }
}

/// Move one item in an ordered list, returning every position that changed.
///
/// Discord takes a whole set of positions rather than a move, and it decides
/// permission conflicts by position - so the intermediate states of a sequence
/// of single moves would briefly hand out the wrong permissions. Sending them
/// together avoids that.
///
/// Generic over the id because roles and channels have the same problem and
/// the same endpoint shape; writing it twice is how the two would drift.
pub fn moved_positions<T: Copy + Eq>(ordered: &[T], index: usize, up: bool) -> Vec<(T, u32)> {
    if index >= ordered.len() {
        return Vec::new();
    }
    let swap_with = if up {
        // Already at the top, so there is nowhere to go. Returning an empty
        // list rather than the unchanged order means no request is sent at
        // all, instead of one that writes the same positions back.
        match index.checked_sub(1) {
            Some(target) => target,
            None => return Vec::new(),
        }
    } else {
        let target = index + 1;
        if target >= ordered.len() {
            return Vec::new();
        }
        target
    };

    let mut moved = ordered.to_vec();
    moved.swap(index, swap_with);

    // Only the two that changed. Sending the whole list would work, but it
    // writes an audit-log entry for every row that did not move.
    moved
        .into_iter()
        .enumerate()
        .filter(|(position, id)| ordered.get(*position) != Some(id))
        .map(|(position, id)| (id, u32::try_from(position).unwrap_or(u32::MAX)))
        .collect()
}

#[cfg(test)]
mod reorder_tests {
    use super::*;

    #[test]
    fn moving_up_swaps_with_the_row_above() {
        let ordered = ['a', 'b', 'c'];
        assert_eq!(moved_positions(&ordered, 1, true), vec![('b', 0), ('a', 1)]);
    }

    #[test]
    fn moving_down_swaps_with_the_row_below() {
        let ordered = ['a', 'b', 'c'];
        assert_eq!(
            moved_positions(&ordered, 1, false),
            vec![('c', 1), ('b', 2)]
        );
    }

    #[test]
    fn only_the_two_rows_that_moved_are_sent() {
        // Sending the whole list works, but writes an audit-log entry for
        // every row that did not move - which buries the one that did.
        let ordered = ['a', 'b', 'c', 'd', 'e'];
        assert_eq!(moved_positions(&ordered, 3, true).len(), 2);
    }

    #[test]
    fn moving_off_either_end_sends_nothing() {
        // No request at all, rather than one that writes the same positions
        // back - which would be an audit entry saying nothing happened.
        let ordered = ['a', 'b', 'c'];
        assert!(moved_positions(&ordered, 0, true).is_empty());
        assert!(moved_positions(&ordered, 2, false).is_empty());
        assert!(moved_positions(&ordered, 9, true).is_empty());
    }

    #[test]
    fn a_single_row_list_cannot_move() {
        let ordered = ['a'];
        assert!(moved_positions(&ordered, 0, true).is_empty());
        assert!(moved_positions(&ordered, 0, false).is_empty());
    }

    #[test]
    fn up_then_down_returns_to_the_original_order() {
        // The two must be exact inverses, or repeated nudging would drift the
        // list somewhere nobody asked for.
        let ordered = ['a', 'b', 'c', 'd'];
        let mut current = ordered.to_vec();
        for (id, position) in moved_positions(&current, 2, true) {
            let from = current
                .iter()
                .position(|c| *c == id)
                .expect("id is present");
            let item = current.remove(from);
            current.insert(position as usize, item);
        }
        assert_eq!(current, vec!['a', 'c', 'b', 'd']);

        for (id, position) in moved_positions(&current, 1, false) {
            let from = current
                .iter()
                .position(|c| *c == id)
                .expect("id is present");
            let item = current.remove(from);
            current.insert(position as usize, item);
        }
        assert_eq!(current, ordered.to_vec());
    }
}

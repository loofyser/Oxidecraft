//! The scoreboard model: objectives, their scores, the display slots, and the
//! teams with their membership.
//!
//! The model is the state the session keeps from clientbound 0x3B–0x3E
//! (`docs/research/protocol-47-reference.md` §2.1) and what the window's
//! sidebar and tab-list surfaces read. The per-mode rules are the source's
//! own handlers (`NetHandlerPlayClient.handleChangeObjective:1870-1894`,
//! `handleUpdateScore:1899-1921`, `handleDisplayScoreboard:1927-1941`,
//! `handleTeams:1947-1997`) over `Scoreboard`'s mutators
//! (`Scoreboard.java:155-368`), and the composition is
//! `ScorePlayerTeam.formatString:95-98`.
//!
//! The colour byte the Teams packet carries is kept as the colour table's
//! index or, for its `-1` sentinel (`0xFF` on the wire, `RESET`'s own index,
//! `EnumChatFormatting.java:45`), as no colour ([`colour_from_wire`]); the
//! composition itself inserts no colour — the source's `formatString` is the
//! prefix and the suffix around the text — and the colour's own consumers
//! are [`entry_colour`]'s callers.

use std::collections::{BTreeMap, BTreeSet};

/// One objective as the model keeps it: the wire's name, display value and
/// render kind (`S3BPacketScoreboardObjective`'s three fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    /// The objective's name, its key.
    pub name: String,
    /// The display value, the wire's objective-value string.
    pub value: String,
    /// The render kind: `integer` or `hearts`.
    pub kind: String,
}

/// One team as the model keeps it: the info block the create and update
/// modes carry, the colour byte's reading, and the membership.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Team {
    /// The team's display name.
    pub display_name: String,
    /// The chat prefix the composition wraps entries with.
    pub prefix: String,
    /// The chat suffix the composition wraps entries with.
    pub suffix: String,
    /// The friendly-fire flags: `0` off, `1` on, `2` see-friendly-invisibles.
    pub friendly_flags: u8,
    /// The name-tag visibility: `always`, `hideForOtherTeams`,
    /// `hideForOwnTeam` or `never`.
    pub name_tag_visibility: String,
    /// The team's colour: a colour-table index `0..=15`, `None` for the
    /// no-colour sentinel and for an index past the table.
    pub colour: Option<u8>,
    /// The team's players.
    pub players: BTreeSet<String>,
}

/// The scoreboard: objectives, their scores, the display slots and the
/// teams, with the membership reverse map.
///
/// The maps are keyed by name, as the wire names them, and the mutators below
/// are the per-mode rules the source's handlers apply. `display` holds the
/// objective names per slot — slot 0 the list, 1 the sidebar, 2 below the
/// name (`docs/research/protocol-47-reference.md` §2.1's slot table) and
/// 3..=18 the sixteen team-coloured sidebar slots of the source's own table
/// (`Scoreboard.java:20`, `getObjectiveDisplaySlotNumber:463-492`); the
/// source holds objective pointers there, and a consumer resolves the name
/// against [`Scoreboard::objectives`] when it draws.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scoreboard {
    /// The objectives, keyed by name.
    pub objectives: BTreeMap<String, Objective>,
    /// The scores: entry name, then objective name, then the value.
    pub scores: BTreeMap<String, BTreeMap<String, i32>>,
    /// The display slots: 0 the list, 1 the sidebar, 2 below the name, and
    /// 3..=18 the sixteen team-coloured sidebar slots — slot `3 + i` for the
    /// team colour index `i` (`Scoreboard.java:20`, `:479-486`).
    pub display: [Option<String>; 19],
    /// The teams, keyed by name.
    pub teams: BTreeMap<String, Team>,
    /// The reverse membership: entry name to team name, kept consistent with
    /// [`Team::players`].
    pub member_of: BTreeMap<String, String>,
}

impl Scoreboard {
    /// An empty board.
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes an objective (`handleChangeObjective`'s create and update
    /// modes, `:1876-1889`; both carry the whole info block and both write
    /// it). A write for a name the board already holds overwrites the value
    /// and the kind and keeps the scores.
    pub fn set_objective(&mut self, name: &str, value: &str, kind: &str) {
        self.objectives.insert(
            name.to_owned(),
            Objective {
                name: name.to_owned(),
                value: value.to_owned(),
                kind: kind.to_owned(),
            },
        );
    }

    /// Removes an objective: it falls, every display slot that named it
    /// clears, and its scores fall too (`removeObjective:216-243`).
    pub fn remove_objective(&mut self, name: &str) {
        self.objectives.remove(name);
        for slot in &mut self.display {
            if slot.as_deref() == Some(name) {
                *slot = None;
            }
        }
        self.scores.retain(|_, by_objective| {
            by_objective.remove(name);
            !by_objective.is_empty()
        });
    }

    /// Writes one score (`getValueFromObjective` and `setScorePoints`,
    /// `handleUpdateScore:1905-1908`).
    pub fn set_score(&mut self, entry: &str, objective: &str, value: i32) {
        self.scores
            .entry(entry.to_owned())
            .or_default()
            .insert(objective.to_owned(), value);
    }

    /// Removes one entry's score for one objective
    /// (`removeObjectiveFromEntity:155-186`).
    pub fn remove_score(&mut self, entry: &str, objective: &str) {
        if let Some(by_objective) = self.scores.get_mut(entry) {
            by_objective.remove(objective);
            if by_objective.is_empty() {
                self.scores.remove(entry);
            }
        }
    }

    /// Removes every score of one entry: the wire's empty objective name
    /// (`handleUpdateScore:1912-1915` reads it as the remove-from-every-
    /// objective signal).
    pub fn remove_scores(&mut self, entry: &str) {
        self.scores.remove(entry);
    }

    /// Sets or clears one display slot (`setObjectiveInDisplaySlot:246-252`;
    /// the empty name clears, `handleDisplayScoreboard:1932-1935`). A slot
    /// outside the nineteen the wire table names is ignored.
    pub fn set_display(&mut self, slot: usize, objective: Option<&str>) {
        if let Some(slot) = self.display.get_mut(slot) {
            *slot = objective.map(str::to_owned);
        }
    }

    /// Writes a team's info block (`handleTeams:1962-1975`; the create and
    /// the update mode both carry it and both write it). An existing team
    /// keeps its membership: the player list arrives with the modes of its
    /// own.
    #[allow(clippy::too_many_arguments)] // The wire's own info block, one field each.
    pub fn set_team(
        &mut self,
        name: &str,
        display_name: &str,
        prefix: &str,
        suffix: &str,
        friendly_flags: u8,
        name_tag_visibility: &str,
        colour: Option<u8>,
    ) {
        let team = self.teams.entry(name.to_owned()).or_default();
        team.display_name = display_name.to_owned();
        team.prefix = prefix.to_owned();
        team.suffix = suffix.to_owned();
        team.friendly_flags = friendly_flags;
        team.name_tag_visibility = name_tag_visibility.to_owned();
        team.colour = colour;
    }

    /// Adds players to a team (`addPlayerToTeam:309-332`): a player on
    /// another team leaves it first, and an add naming a team the board does
    /// not hold is ignored.
    pub fn add_team_players(&mut self, team: &str, players: &[String]) {
        if !self.teams.contains_key(team) {
            return;
        }
        for player in players {
            // The source removes the old membership before the put
            // (`:323-329`); a re-add to the same team is no move.
            if let Some(previous) = self.member_of.get(player).cloned() {
                if previous != team {
                    if let Some(previous) = self.teams.get_mut(&previous) {
                        previous.players.remove(player);
                    }
                }
            }
            self.member_of.insert(player.clone(), team.to_owned());
            if let Some(row) = self.teams.get_mut(team) {
                row.players.insert(player.clone());
            }
        }
    }

    /// Removes players from a team. The source removes only a player the
    /// team actually holds and refuses the others
    /// (`removePlayerFromTeam:353-359`); the model ignores those rows, and a
    /// player on another team is untouched.
    pub fn remove_team_players(&mut self, team: &str, players: &[String]) {
        for player in players {
            if self.member_of.get(player).map(String::as_str) != Some(team) {
                continue;
            }
            self.member_of.remove(player);
            if let Some(row) = self.teams.get_mut(team) {
                row.players.remove(player);
            }
        }
    }

    /// Removes a team and every membership that named it
    /// (`removeTeam:296-304`).
    pub fn remove_team(&mut self, name: &str) {
        self.teams.remove(name);
        self.member_of.retain(|_, team| team != name);
    }

    /// The team an entry belongs to, if any (`getPlayersTeam:379-381`).
    pub fn team_of(&self, entry: &str) -> Option<&Team> {
        self.member_of
            .get(entry)
            .and_then(|team| self.teams.get(team))
    }
}

/// The Teams packet's colour byte as this model keeps it.
///
/// The wire carries `EnumChatFormatting.getColorIndex` — the colour table's
/// index, or `-1` (`0xFF` on the wire) for the no-colour sentinel
/// (`S3EPacketTeams.java:46,92,121`) — and the source reads it back through
/// `func_175744_a` (`EnumChatFormatting.java:136-152`), which answers the
/// sentinel with `RESET` and an index past the fifteen-colour table with
/// nothing. Both read as no colour here; the indices inside the table read
/// as the colour they name.
pub fn colour_from_wire(byte: u8) -> Option<u8> {
    if byte <= 15 { Some(byte) } else { None }
}

/// The composed text for an entry: the team's clauses around the fallback.
///
/// `ScorePlayerTeam.formatString:95-98` is `prefix + input + suffix` and is
/// what the tab list (`GuiPlayerTabOverlay.getPlayerName:45-51`), the
/// sidebar (`GuiIngame.java:577`, `:591`) and a player's nametag
/// (`EntityPlayer.getDisplayName:2316-2324`) all read, through
/// `formatPlayerName:103-106`. The team's colour byte is not part of the
/// text: the source keeps it as the team's chat format
/// (`handleTeams:1967`) and no text composition reads it — the surfaces
/// that colour a name from it read [`entry_colour`].
pub fn format_entry(board: &Scoreboard, entry: &str, fallback: &str) -> String {
    match board.team_of(entry) {
        Some(team) => format!("{}{}{}", team.prefix, fallback, team.suffix),
        None => fallback.to_owned(),
    }
}

/// The colour of the team an entry belongs to, for the surfaces that colour
/// from it. `None` when the entry has no team, and when the team carries the
/// no-colour sentinel.
pub fn entry_colour(board: &Scoreboard, entry: &str) -> Option<u8> {
    board.team_of(entry).and_then(|team| team.colour)
}

#[cfg(test)]
mod tests {
    //! The model's per-mode rules and the composition fixtures: every value
    //! is the literal the source's handlers write
    //! (`NetHandlerPlayClient.java:1870-1997`), the membership rules follow
    //! `Scoreboard.java`'s three mutators (`:296-368`), and the composition
    //! fixtures pin `ScorePlayerTeam.formatString` (`:95-98`).

    use super::{Objective, Scoreboard, colour_from_wire, entry_colour, format_entry};

    #[test]
    fn an_objective_is_written_and_a_remove_drops_it_with_its_slots_and_scores() {
        let mut board = Scoreboard::new();
        board.set_objective("kills", "Kills", "integer");
        assert_eq!(
            board.objectives.get("kills"),
            Some(&Objective {
                name: "kills".to_owned(),
                value: "Kills".to_owned(),
                kind: "integer".to_owned(),
            }),
            "the create mode's three fields"
        );
        // The update mode carries the same info block and rewrites it
        // (`handleChangeObjective`'s mode 2, `NetHandlerPlayClient.java:1884-1889`;
        // `ScoreObjective.setDisplayName:42-45`, `setRenderType:53-56`).
        board.set_objective("kills", "Kills remastered", "hearts");
        assert_eq!(
            (
                board.objectives["kills"].value.as_str(),
                board.objectives["kills"].kind.as_str(),
            ),
            ("Kills remastered", "hearts"),
            "the update rewrites the value and the kind"
        );
        // A remove drops the objective, every display slot that named it and
        // its scores (`Scoreboard.removeObjective:216-243`).
        board.set_display(1, Some("kills"));
        board.set_score("Alpha", "kills", 5);
        board.remove_objective("kills");
        assert!(board.objectives.is_empty(), "the objective fell");
        assert!(
            board.display.iter().all(Option::is_none),
            "the slot that named it cleared: {:?}",
            board.display
        );
        assert!(
            board.scores.is_empty(),
            "its scores fell with it: {:?}",
            board.scores
        );
        // A remove of an objective the board never held changes nothing.
        let empty = Scoreboard::new();
        let mut repeated = Scoreboard::new();
        repeated.remove_objective("kills");
        assert_eq!(repeated, empty, "an unknown remove is a no-op");
    }

    #[test]
    fn a_score_is_set_and_removed_per_objective() {
        let mut board = Scoreboard::new();
        board.set_score("Alpha", "kills", 5);
        board.set_score("Beta", "kills", 7);
        board.set_score("Alpha", "deaths", 1);
        assert_eq!(board.scores["Alpha"]["kills"], 5);
        // The set mode is a write, not an add.
        board.set_score("Alpha", "kills", 9);
        assert_eq!(board.scores["Alpha"]["kills"], 9);
        // The remove mode drops one pair (`removeObjectiveFromEntity:155-186`).
        board.remove_score("Alpha", "kills");
        assert!(!board.scores["Alpha"].contains_key("kills"));
        assert_eq!(
            board.scores["Alpha"]["deaths"], 1,
            "the other objective stays"
        );
        assert_eq!(board.scores["Beta"]["kills"], 7, "the other entry stays");
        // The empty-objective remove drops every pair of the entry
        // (`handleUpdateScore:1912-1915`).
        board.remove_scores("Alpha");
        assert!(
            !board.scores.contains_key("Alpha"),
            "the entry's map went with it"
        );
        // A remove that names nothing held changes nothing.
        let before = board.clone();
        board.remove_score("Gamma", "kills");
        board.remove_scores("Gamma");
        assert_eq!(board, before);
    }

    #[test]
    fn the_display_slots_set_and_clear_by_slot() {
        let mut board = Scoreboard::new();
        board.set_display(0, Some("list"));
        board.set_display(1, Some("side"));
        board.set_display(2, Some("below"));
        assert_eq!(board.display[0], Some("list".to_owned()));
        assert_eq!(board.display[1], Some("side".to_owned()));
        assert_eq!(board.display[2], Some("below".to_owned()));
        // The empty name clears the slot (`handleDisplayScoreboard:1932-1935`;
        // the decoder carries the empty name as `None`).
        board.set_display(1, None);
        assert_eq!(board.display[1], None);
        // A slot outside the source's own array of nineteen
        // (`Scoreboard.java:20`) is ignored, not a panic.
        board.set_display(19, Some("nowhere"));
        board.set_display(usize::MAX, Some("nowhere"));
        assert_eq!(board.display[2], Some("below".to_owned()));
        assert_eq!(board.display[0], Some("list".to_owned()));
    }

    #[test]
    fn the_team_display_slots_store_and_clear() {
        // Slots 3..=18 are the sixteen team-coloured sidebar slots of the
        // source's own table (`Scoreboard.java:20`): slot `3 + i` for the
        // colour index `i` (`getObjectiveDisplaySlotNumber:479-486`).
        let mut board = Scoreboard::new();
        assert_eq!(board.display.len(), 19, "the source's own array width");
        board.set_display(3, Some("red-side"));
        board.set_display(18, Some("white-side"));
        assert_eq!(board.display[3], Some("red-side".to_owned()));
        assert_eq!(board.display[18], Some("white-side".to_owned()));
        // The empty name clears one (`handleDisplayScoreboard:1932-1935`).
        board.set_display(3, None);
        assert_eq!(board.display[3], None);
        assert_eq!(
            board.display[18],
            Some("white-side".to_owned()),
            "the other team slot stays"
        );
    }

    #[test]
    fn a_team_writes_its_info_block_and_keeps_its_players_across_an_update() {
        let mut board = Scoreboard::new();
        board.set_team("red", "Red Team", "§c[Red] ", "§r", 1, "always", Some(12));
        let team = &board.teams["red"];
        assert_eq!(team.display_name, "Red Team");
        assert_eq!(team.prefix, "§c[Red] ");
        assert_eq!(team.suffix, "§r");
        assert_eq!(team.friendly_flags, 1);
        assert_eq!(team.name_tag_visibility, "always");
        assert_eq!(team.colour, Some(12), "red is colour index 12");
        // The create mode's player list arrives with the add-players handling
        // (`handleTeams:1977-1983` runs for the create and the add modes).
        board.add_team_players("red", &["Alpha".to_owned()]);
        // The update mode rewrites the info block and carries no player list
        // (`handleTeams:1962-1975` runs for the create and the update modes).
        board.set_team("red", "Red", "", "", 2, "never", None);
        let team = &board.teams["red"];
        assert_eq!(
            (team.display_name.as_str(), team.friendly_flags),
            ("Red", 2)
        );
        assert_eq!(team.name_tag_visibility, "never");
        assert_eq!(team.colour, None);
        assert!(
            team.players.contains("Alpha"),
            "the update carried no player list: {:?}",
            team.players
        );
        assert_eq!(board.member_of["Alpha"], "red");
    }

    #[test]
    fn a_player_belongs_to_one_team_and_moves_between_them() {
        let mut board = Scoreboard::new();
        board.set_team("red", "Red", "", "", 0, "always", Some(12));
        board.set_team("blue", "Blue", "", "", 0, "always", Some(9));
        board.add_team_players("red", &["Alpha".to_owned()]);
        assert!(board.teams["red"].players.contains("Alpha"));
        assert_eq!(board.member_of["Alpha"], "red");
        // Adding to another team leaves the first (`addPlayerToTeam:323-329`
        // drops the old membership before the put).
        board.add_team_players("blue", &["Alpha".to_owned()]);
        assert_eq!(board.member_of["Alpha"], "blue");
        assert!(
            !board.teams["red"].players.contains("Alpha"),
            "the old team let the player go: {:?}",
            board.teams["red"].players
        );
        // An add naming a team the board does not hold is refused silently
        // (`addPlayerToTeam:315-318` answers false for an unknown name).
        let before = board.clone();
        board.add_team_players("ghost", &["Alpha".to_owned()]);
        assert_eq!(board, before, "no membership for an unknown team");
        // A re-add to the same team keeps the membership (`:323-329` removes
        // and re-adds; the state is the same membership).
        board.add_team_players("blue", &["Alpha".to_owned()]);
        assert!(board.teams["blue"].players.contains("Alpha"));
        assert_eq!(board.member_of["Alpha"], "blue");
    }

    #[test]
    fn a_remove_players_leaves_only_the_team_the_player_is_on() {
        let mut board = Scoreboard::new();
        board.set_team("red", "Red", "", "", 0, "always", Some(12));
        board.set_team("blue", "Blue", "", "", 0, "always", Some(9));
        board.add_team_players("blue", &["Alpha".to_owned()]);
        // The source's remove-from-team refuses a player who is not on that
        // team (`removePlayerFromTeam:353-359` throws); the model ignores it
        // and only the team the player is actually on loses them.
        board.remove_team_players("red", &["Alpha".to_owned()]);
        assert_eq!(board.member_of["Alpha"], "blue");
        assert!(board.teams["red"].players.is_empty());
        board.remove_team_players("blue", &["Alpha".to_owned()]);
        assert!(!board.member_of.contains_key("Alpha"));
        assert!(board.teams["blue"].players.is_empty());
        // Removing a player on no team changes nothing.
        let before = board.clone();
        board.remove_team_players("blue", &["Nobody".to_owned()]);
        board.remove_team_players("ghost", &["Alpha".to_owned()]);
        assert_eq!(board, before);
    }

    #[test]
    fn removing_a_team_drops_every_membership_with_it() {
        let mut board = Scoreboard::new();
        board.set_team("red", "Red", "", "", 0, "always", Some(12));
        board.set_team("blue", "Blue", "", "", 0, "always", Some(9));
        board.add_team_players("red", &["Alpha".to_owned(), "Beta".to_owned()]);
        board.add_team_players("blue", &["Gamma".to_owned()]);
        board.remove_team("red");
        assert!(!board.teams.contains_key("red"));
        assert!(!board.member_of.contains_key("Alpha"));
        assert!(!board.member_of.contains_key("Beta"));
        assert_eq!(
            board.member_of["Gamma"], "blue",
            "the other team's member stays"
        );
        // An unknown remove changes nothing.
        let before = board.clone();
        board.remove_team("ghost");
        assert_eq!(board, before);
    }

    #[test]
    fn the_composition_is_the_prefix_the_text_and_the_suffix() {
        let mut board = Scoreboard::new();
        board.set_team("red", "Red", "§c[Red] ", "§r", 1, "always", Some(12));
        board.add_team_players("red", &["Alpha".to_owned()]);
        // `ScorePlayerTeam.formatString:95-98` is `prefix + input + suffix`,
        // and it is what every text surface reads
        // (`formatPlayerName:103-106` reaches it). The colour byte is not
        // part of the text: the source keeps it as the team's chat format
        // (`handleTeams:1967`), and no `getFormattedText` call site composes
        // it into the name.
        assert_eq!(
            format_entry(&board, "Alpha", "Alpha"),
            "§c[Red] Alpha§r",
            "the team's prefix and suffix wrap the text"
        );
        // The fallback is the text an entry with no team of its own gets.
        assert_eq!(format_entry(&board, "Nobody", "Nobody"), "Nobody");
        // A team whose clauses are empty adds nothing — including a team
        // with a colour set: no colour code joins the text.
        board.set_team("plain", "Plain", "", "", 0, "always", Some(12));
        board.add_team_players("plain", &["Beta".to_owned()]);
        assert_eq!(
            format_entry(&board, "Beta", "§bBeta"),
            "§bBeta",
            "the colour byte does not enter the text"
        );
        // A suffix-only team.
        board.set_team("tail", "Tail", "", "§r", 0, "always", None);
        board.add_team_players("tail", &["Gamma".to_owned()]);
        assert_eq!(format_entry(&board, "Gamma", "Gamma"), "Gamma§r");
        // The fallback can be a display name that is itself coded; it is
        // wrapped as the text it is.
        assert_eq!(format_entry(&board, "Gamma", "§bOx"), "§bOx§r");
    }

    #[test]
    fn the_team_colour_is_the_byte_the_source_keeps_and_the_sentinel_maps_to_none() {
        // The colour travels as `EnumChatFormatting.getColorIndex` on the way
        // out and `func_175744_a` on the way in
        // (`S3EPacketTeams.java:46,92,121`): `-1` — `0xFF` on the wire — is
        // the sentinel for no colour (RESET's own index,
        // `EnumChatFormatting.java:45`), and an index past the fifteen-colour
        // table names no colour either (`func_175744_a:136-152` answers
        // `null`).
        assert_eq!(colour_from_wire(0xff), None, "the sentinel is no colour");
        assert_eq!(colour_from_wire(12), Some(12), "red");
        assert_eq!(colour_from_wire(0), Some(0), "black");
        assert_eq!(colour_from_wire(15), Some(15), "white");
        assert_eq!(colour_from_wire(16), None, "past the colour table");

        let mut board = Scoreboard::new();
        board.set_team("red", "Red", "", "", 0, "always", colour_from_wire(0xff));
        board.add_team_players("red", &["Alpha".to_owned()]);
        assert_eq!(
            entry_colour(&board, "Alpha"),
            None,
            "a sentinel team has no colour to give"
        );
        board.set_team("red", "Red", "", "", 0, "always", Some(12));
        assert_eq!(entry_colour(&board, "Alpha"), Some(12));
        assert_eq!(entry_colour(&board, "Nobody"), None);
    }
}

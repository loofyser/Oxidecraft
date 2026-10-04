//! The player state the session's ticks carry.

use oxide_proto_v47::clientbound::PlayerAbilities;

use crate::input::SprintTap;

/// The standing eye height above the player's feet, in blocks.
///
/// `EntityPlayer.getEyeHeight` (`EntityPlayer.java:2326-2340`) bases the eye
/// on `float f = 1.62F`. The source lowers it to 0.2 while sleeping and by
/// 0.08 while sneaking (`:2330-2337`); M3 carries the standing value, and the
/// sleeping eye arrives with M6's screens.
const EYE_HEIGHT: f64 = 1.62;

/// The default flight speed: `private float flySpeed = 0.05F`
/// (`PlayerCapabilities.java:23`).
const FLY_SPEED: f32 = 0.05;

/// The default walking speed: `private float walkSpeed = 0.1F`
/// (`PlayerCapabilities.java:24`).
const WALK_SPEED: f32 = 0.1;

/// The double-tap window of the flight toggle, in ticks:
/// `this.flyToggleTimer = 7` (`EntityPlayerSP.java:837`).
const FLY_TOGGLE_WINDOW: i32 = 7;

/// The player's maximum health, which a fresh `EntityLivingBase` starts at:
/// `setHealth(getMaxHealth())` (`EntityLivingBase.java:201`) over
/// `maxHealth`'s base value 20.0 (`SharedMonsterAttributes.java:18`).
const MAX_HEALTH: f32 = 20.0;

/// `FoodStats`' starting food level: `private int foodLevel = 20`
/// (`FoodStats.java:12`).
const START_FOOD: i32 = 20;

/// `FoodStats`' starting saturation: `private float foodSaturationLevel = 5.0F`
/// (`FoodStats.java:15`).
const START_SATURATION: f32 = 5.0;

/// The hurt flash's length in ticks: the status-2 handler sets
/// `hurtTime = maxHurtTime = 10` (`EntityLivingBase.handleStatusUpdate:1362`).
pub const MAX_HURT_TIME: u32 = 10;

/// The player's abilities, the state the ability packets carry.
///
/// The fields are `PlayerCapabilities`' own (`PlayerCapabilities.java:7-24`):
/// the four flags the flags byte of 0x13/0x39 packs and the two speeds. The
/// defaults are the source's field initialisers — every flag false, `0.05F`
/// fly speed and `0.1F` walk speed — because the client holds nothing until a
/// 0x39 arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Abilities {
    /// Whether the player is flying (`isFlying`).
    pub flying: bool,
    /// Whether flight may be toggled by the double-tap (`allowFlying`).
    pub allow_flying: bool,
    /// Whether creative mode is on (`isCreativeMode`).
    pub creative: bool,
    /// Whether damage is disabled (`disableDamage`).
    pub invulnerable: bool,
    /// The flight speed.
    pub fly_speed: f32,
    /// The walking speed.
    pub walk_speed: f32,
}

impl Default for Abilities {
    fn default() -> Self {
        Self {
            flying: false,
            allow_flying: false,
            creative: false,
            invulnerable: false,
            fly_speed: FLY_SPEED,
            walk_speed: WALK_SPEED,
        }
    }
}

impl Abilities {
    /// The flags byte both ability packets carry.
    ///
    /// The bits are `C13PacketPlayerAbilities.writePacketData`'s own
    /// (`C13PacketPlayerAbilities.java:66-71`), read from the packet's own
    /// constants so the two sides cannot drift.
    pub fn flags(&self) -> u8 {
        let mut flags = 0;
        if self.invulnerable {
            flags |= PlayerAbilities::FLAG_INVULNERABLE;
        }
        if self.flying {
            flags |= PlayerAbilities::FLAG_FLYING;
        }
        if self.allow_flying {
            flags |= PlayerAbilities::FLAG_ALLOW_FLYING;
        }
        if self.creative {
            flags |= PlayerAbilities::FLAG_CREATIVE;
        }
        flags
    }
}

/// The player: the state one tick steps and the window draws.
///
/// The fields are the surface the movement and physics rules extend.
/// `position`, `last_tick_position`, `tick`, the look and the flags are what
/// the per-tick `PlayerTick` event reports; `motion` and `on_ground` are the
/// physics core's input; `flying` and `in_water` gate the movement rules;
/// `abilities` is the server's own statement of flight and the speeds it
/// sets. `last_reported_*`, `position_update_ticks`, `server_sprint_state`
/// and `server_sneak_state` are the walking report's state, the source's
/// `lastReported*`/`positionUpdateTicks` and `serverSprintState`/
/// `serverSneakState` (`EntityPlayerSP.java:64-100`, `:189-274`). `health`,
/// `food` and `saturation` mirror the last 0x06, `dead`/`death_time` carry
/// the death state and `hurt_time`/`attacked_at_yaw` the hurt state.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// The feet position in the world.
    pub position: [f64; 3],
    /// The feet position at the previous tick.
    pub last_tick_position: [f64; 3],
    /// Look yaw in degrees.
    pub yaw: f32,
    /// Look pitch in degrees, positive looking down.
    pub pitch: f32,
    /// Velocity in blocks per tick.
    pub motion: [f64; 3],
    /// Whether the player stands on something.
    pub on_ground: bool,
    /// Whether the player is sprinting.
    pub sprinting: bool,
    /// Whether the player is sneaking.
    pub sneaking: bool,
    /// Whether the player is flying.
    pub flying: bool,
    /// Whether the player is in water.
    pub in_water: bool,
    /// Ticks left before the held jump may fire again.
    ///
    /// `EntityLivingBase.onLivingUpdate:1949-1952` counts `jumpTicks` down and
    /// the held-jump rule (`:2007-2027`) fires only at zero, then sets it to
    /// ten: Space held makes the player jump once per ten ticks.
    pub jump_ticks: i32,
    /// Ticks since the session started.
    pub tick: u64,
    /// The double-tap window the sprint binding keeps across ticks.
    pub sprint_tap: SprintTap,
    /// The player's abilities, from 0x39; the flags byte's own fields.
    pub abilities: Abilities,
    /// The player's own entity id, named by Join Game.
    ///
    /// The entity-action packets carry it (`C0BPacketEntityAction`), and its
    /// arrival is where this client's player starts reporting — before Join
    /// Game there is no world and no id to report under.
    pub entity_id: Option<i32>,
    /// The feet position the walking report last sent (`lastReportedPosX/Y/Z`).
    pub last_reported_position: [f64; 3],
    /// The yaw the walking report last sent (`lastReportedYaw`).
    pub last_reported_yaw: f32,
    /// The pitch the walking report last sent (`lastReportedPitch`).
    pub last_reported_pitch: f32,
    /// Ticks since the last position report (`positionUpdateTicks`).
    pub position_update_ticks: i32,
    /// The sprint state the server was last told (`serverSprintState`).
    pub server_sprint_state: bool,
    /// The sneak state the server was last told (`serverSneakState`).
    pub server_sneak_state: bool,
    /// Ticks left in the flight toggle's double-tap window (`flyToggleTimer`).
    pub fly_toggle_timer: i32,
    /// The jump bit the previous tick read.
    ///
    /// `EntityPlayerSP.onLivingUpdate` reads `movementInput.jump` before the
    /// tick's input refresh (`EntityPlayerSP.java:781`) and calls the jump a
    /// fresh press when it was up then and is down now (`:834`); this is that
    /// previous bit.
    pub prev_jump: bool,
    /// The player's health, from clientbound 0x06.
    ///
    /// A fresh `EntityLivingBase` starts at its maximum
    /// (`EntityLivingBase.java:201` calls `setHealth(getMaxHealth())`), which
    /// for a player is 20.0 (`SharedMonsterAttributes.java:18`); the server's
    /// 0x06 replaces it, and a value at or below zero is the death.
    pub health: f32,
    /// The food level, from clientbound 0x06; `FoodStats`' own start is 20
    /// (`FoodStats.java:12`).
    pub food: i32,
    /// The food saturation, from clientbound 0x06; `FoodStats`' own start is
    /// 5.0 (`FoodStats.java:15`).
    pub saturation: f32,
    /// Whether the player is dead: health at or below zero, from the 0x06
    /// that landed it, until clientbound 0x07 respawns the player.
    ///
    /// The source reads the death straight off the health each tick
    /// (`EntityLivingBase.onEntityUpdate:344-349`); this client keeps the flag
    /// its own 0x06 set, because the health mirror is not re-synced until the
    /// server sends the next 0x06.
    pub dead: bool,
    /// Ticks since the death state began (`deathTime`).
    ///
    /// The source advances it once per tick while the entity is dead
    /// (`EntityLivingBase.onDeathUpdate:398-400`, reached from
    /// `onEntityUpdate:349`), and the camera's death tilt reads it (Task 11).
    pub death_time: u32,
    /// Ticks left of the hurt flash (`hurtTime`).
    ///
    /// The hurt status sets it to its maximum, ten
    /// (`EntityLivingBase.handleStatusUpdate:1362`), and it counts down once
    /// per tick (`onEntityUpdate:337-340`). Task 11's hurt roll reads it.
    pub hurt_time: u32,
    /// The yaw the last hurt came from (`attackedAtYaw`), for the hurt roll.
    ///
    /// The client's own status-2 handler zeroes it
    /// (`EntityLivingBase.handleStatusUpdate:1363`); the attacker-derived value
    /// is computed server-side in `attackEntityFrom` (`:961`) from the
    /// attacker's position and is never carried by the status packet, so this
    /// stays zero until M4 tracks attackers.
    pub attacked_at_yaw: f32,
}

impl Player {
    /// A new player at the origin, looking south, nothing held.
    pub fn new() -> Player {
        Player {
            position: [0.0; 3],
            last_tick_position: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            motion: [0.0; 3],
            on_ground: false,
            sprinting: false,
            sneaking: false,
            flying: false,
            in_water: false,
            jump_ticks: 0,
            tick: 0,
            sprint_tap: SprintTap::default(),
            abilities: Abilities::default(),
            entity_id: None,
            last_reported_position: [0.0; 3],
            last_reported_yaw: 0.0,
            last_reported_pitch: 0.0,
            position_update_ticks: 0,
            server_sprint_state: false,
            server_sneak_state: false,
            fly_toggle_timer: 0,
            prev_jump: false,
            health: MAX_HEALTH,
            food: START_FOOD,
            saturation: START_SATURATION,
            dead: false,
            death_time: 0,
            hurt_time: 0,
            attacked_at_yaw: 0.0,
        }
    }

    /// The eye's height above the feet, in blocks.
    ///
    /// The one place the constant is defined; the interaction raycast and the
    /// camera read the value from here.
    pub fn eye_height(&self) -> f64 {
        EYE_HEIGHT
    }

    /// Sets the flight state, both copies of it.
    ///
    /// `flying` is the flag the movement model reads (`physics::step`) and
    /// `abilities.flying` is the same fact as the ability packets carry it;
    /// every writer goes through here, so the two cannot disagree.
    pub fn set_flying(&mut self, flying: bool) {
        self.flying = flying;
        self.abilities.flying = flying;
    }

    /// Applies a clientbound 0x39.
    ///
    /// The source's handler replaces every ability field and takes the
    /// packet's own flying value, true or false
    /// (`NetHandlerPlayClient.handlePlayerAbilities`,
    /// `NetHandlerPlayClient.java:1674-1683`).
    pub fn apply_abilities(&mut self, abilities: Abilities) {
        self.abilities = abilities;
        self.flying = abilities.flying;
    }

    /// One tick of the double-tap flight toggle, and the window's countdown.
    ///
    /// The toggle is `EntityPlayerSP.onLivingUpdate`'s own (`:823-845`): with
    /// flight allowed, a fresh jump press — down this tick, up the previous
    /// one (`!flag && movementInput.jump`, `:834`) — arms the seven-tick
    /// window, and a second fresh press inside it flips `flying`, returning
    /// `true` so the caller sends 0x13. The toggle has no ground gate: the
    /// source flips flight anywhere (`:833-845`). The spectator branch
    /// (`:825-832`) is not ported — M3 has no spectator mode. The countdown
    /// runs every tick after the toggle check, exactly where the source's
    /// `super.onLivingUpdate` runs it (`EntityPlayer.onLivingUpdate`,
    /// `EntityPlayer.java:599-602`).
    pub fn update_flight(&mut self, jump: bool) -> bool {
        let fresh_press = !self.prev_jump && jump;
        self.prev_jump = jump;
        let mut toggled = false;
        if self.abilities.allow_flying && fresh_press {
            if self.fly_toggle_timer == 0 {
                self.fly_toggle_timer = FLY_TOGGLE_WINDOW;
            } else {
                self.set_flying(!self.flying);
                self.fly_toggle_timer = 0;
                toggled = true;
            }
        }
        if self.fly_toggle_timer > 0 {
            self.fly_toggle_timer -= 1;
        }
        toggled
    }

    /// Applies the hurt status (clientbound 0x1A with status
    /// [`EntityStatus::HURT`](oxide_proto_v47::clientbound::EntityStatus::HURT)).
    ///
    /// The source's status-2 branch sets `hurtTime = maxHurtTime = 10` and
    /// zeroes `attackedAtYaw` (`EntityLivingBase.handleStatusUpdate:1362-1363`).
    /// The attacker-derived yaw — `attackEntityFrom` computes it from the
    /// attacker's position (`:961`) — is server-side state that the status
    /// packet does not carry, so the field stays zero until M4 tracks
    /// attackers; that split is recorded on [`Player::attacked_at_yaw`].
    pub fn apply_hurt_status(&mut self) {
        self.hurt_time = MAX_HURT_TIME;
        self.attacked_at_yaw = 0.0;
    }

    /// The player-local half of a respawn reset (clientbound 0x07).
    ///
    /// The source answers a respawn by building a fresh `EntityPlayerSP` and
    /// spawning it in (`Minecraft.setDimensionAndSpawnPlayer:990-1018`): the
    /// entity id, the position and the client brand carry over, every other
    /// transient field starts over, and `preparePlayerToSpawn` zeroes the
    /// motion and the pitch (`Entity.preparePlayerToSpawn:315-333`), resets the
    /// health to its maximum and the death clock to zero
    /// (`EntityPlayer.preparePlayerToSpawn:578-583`). This port keeps the
    /// session's own object and clears the same transients in place: the
    /// motion, the pitch, the movement flags and their server mirrors, the
    /// ground, water, jump and flight-toggle state, the hurt flash and the
    /// death state. The position — which the source only nudges upward out of
    /// a collision — is left to the 0x08 that follows, which carries it
    /// absolutely.
    pub fn reset_for_respawn(&mut self) {
        self.motion = [0.0; 3];
        self.pitch = 0.0;
        self.health = MAX_HEALTH;
        self.dead = false;
        self.death_time = 0;
        self.hurt_time = 0;
        self.attacked_at_yaw = 0.0;
        self.sprinting = false;
        self.sneaking = false;
        self.server_sprint_state = false;
        self.server_sneak_state = false;
        // The flags follow the abilities the server last stated, so the two
        // copies cannot disagree (`Player::set_flying`).
        self.set_flying(self.abilities.flying);
        self.on_ground = false;
        self.in_water = false;
        self.jump_ticks = 0;
        self.fly_toggle_timer = 0;
        self.prev_jump = false;
        self.sprint_tap = SprintTap::default();
    }
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

#[cfg(test)]
mod tests {
    //! The pinned literals of the player state.

    use super::{Abilities, MAX_HURT_TIME, Player};

    #[test]
    fn a_new_player_is_at_the_origin_and_the_eye_is_the_sources_own_height() {
        let player = Player::new();
        assert_eq!(player.position, [0.0, 0.0, 0.0]);
        assert_eq!(player.last_tick_position, [0.0, 0.0, 0.0]);
        assert_eq!(player.motion, [0.0, 0.0, 0.0]);
        assert_eq!(player.tick, 0);
        assert_eq!(player.jump_ticks, 0);
        assert!(!player.on_ground);
        assert!(!player.sprinting);
        assert!(!player.sneaking);
        assert!(!player.flying);
        assert!(!player.in_water);
        // `EntityPlayer.java:2326-2340`: `float f = 1.62F`.
        assert_eq!(player.eye_height(), 1.62);
        // The abilities start at the source's field initialisers
        // (`PlayerCapabilities.java:23-24`) with every flag false: nothing
        // arrives until a 0x39 does.
        assert_eq!(player.abilities, Abilities::default());
        assert_eq!(player.abilities.fly_speed, 0.05);
        assert_eq!(player.abilities.walk_speed, 0.1);
        // The walking report starts at the origin it reports from, and the
        // edge state starts telling the server nothing.
        assert_eq!(player.entity_id, None);
        assert_eq!(player.last_reported_position, [0.0; 3]);
        assert_eq!(player.last_reported_yaw, 0.0);
        assert_eq!(player.last_reported_pitch, 0.0);
        assert_eq!(player.position_update_ticks, 0);
        assert!(!player.server_sprint_state);
        assert!(!player.server_sneak_state);
        assert_eq!(player.fly_toggle_timer, 0);
        assert!(!player.prev_jump);
        // A fresh entity starts at its maximum health
        // (`EntityLivingBase.java:201`, `SharedMonsterAttributes.java:18`),
        // with `FoodStats`' own starting level and saturation
        // (`FoodStats.java:12`, `:15`), and neither dead nor hurt.
        assert_eq!(player.health, 20.0);
        assert_eq!(player.food, 20);
        assert_eq!(player.saturation, 5.0);
        assert!(!player.dead);
        assert_eq!(player.death_time, 0);
        assert_eq!(player.hurt_time, 0);
        assert_eq!(player.attacked_at_yaw, 0.0);
    }

    #[test]
    fn the_hurt_status_sets_the_maximum_hurt_time_and_zeroes_the_attacker_yaw() {
        // `EntityLivingBase.handleStatusUpdate:1362-1363`: status 2 sets
        // `hurtTime = maxHurtTime = 10` and `attackedAtYaw = 0.0F`.
        assert_eq!(MAX_HURT_TIME, 10, "the source's own ten ticks");
        let mut player = Player::new();
        player.hurt_time = 3;
        player.attacked_at_yaw = 42.0;
        player.apply_hurt_status();
        assert_eq!(player.hurt_time, MAX_HURT_TIME);
        assert_eq!(player.attacked_at_yaw, 0.0);
    }

    #[test]
    fn the_respawn_reset_clears_the_transients_and_keeps_the_abilities() {
        // `Minecraft.setDimensionAndSpawnPlayer:990-1018` with
        // `EntityPlayer.preparePlayerToSpawn:578-583`: motion, pitch, the hurt
        // flash, the death state and every movement transient start over; the
        // health returns to its maximum; the abilities the server last stated
        // are the one thing the flags still follow.
        let mut player = Player::new();
        player.motion = [1.0, 2.0, 3.0];
        player.pitch = -30.0;
        player.yaw = 90.0;
        player.health = 0.0;
        player.dead = true;
        player.death_time = 17;
        player.hurt_time = 4;
        player.attacked_at_yaw = 12.0;
        player.sprinting = true;
        player.sneaking = true;
        player.server_sprint_state = true;
        player.server_sneak_state = true;
        player.on_ground = true;
        player.in_water = true;
        player.jump_ticks = 6;
        player.fly_toggle_timer = 5;
        player.prev_jump = true;
        player.abilities.allow_flying = true;
        // A flight left over from before the death: the abilities say the
        // player is not flying, so the reset must follow them, not the stale
        // flag.
        player.flying = true;
        player.reset_for_respawn();
        assert_eq!(player.motion, [0.0; 3]);
        assert_eq!(player.pitch, 0.0, "`preparePlayerToSpawn` zeroes the pitch");
        assert_eq!(player.yaw, 90.0, "the yaw the source does not touch");
        assert_eq!(player.health, 20.0, "the health returns to its maximum");
        assert!(!player.dead);
        assert_eq!(player.death_time, 0);
        assert_eq!(player.hurt_time, 0);
        assert_eq!(player.attacked_at_yaw, 0.0);
        assert!(!player.sprinting);
        assert!(!player.sneaking);
        assert!(!player.server_sprint_state);
        assert!(!player.server_sneak_state);
        assert!(!player.flying, "the abilities' own flying value is false");
        assert!(!player.abilities.flying, "both copies agree");
        assert!(!player.on_ground);
        assert!(!player.in_water);
        assert_eq!(player.jump_ticks, 0);
        assert_eq!(player.fly_toggle_timer, 0);
        assert!(!player.prev_jump);
        assert!(player.abilities.allow_flying, "the abilities are kept");

        // With flying stated by the abilities, the reset follows them.
        let mut player = Player::new();
        let flying_abilities = Abilities {
            flying: true,
            allow_flying: true,
            ..Abilities::default()
        };
        player.apply_abilities(flying_abilities);
        player.set_flying(true);
        player.reset_for_respawn();
        assert!(player.flying && player.abilities.flying);
    }

    #[test]
    fn the_flight_toggle_arms_and_flips_inside_its_window() {
        // `EntityPlayerSP.onLivingUpdate:823-845`: the first fresh press arms
        // the window, a second fresh press inside it flips flight, and the
        // countdown runs every tick after the check.
        let mut player = Player::new();
        player.abilities.allow_flying = true;
        // One fresh press arms: `flyToggleTimer = 7` (`:837`), then the
        // tick's own countdown (`EntityPlayer.java:599-602`).
        assert!(!player.update_flight(true), "arming is not a flip");
        assert_eq!(player.fly_toggle_timer, 6, "seven set, one tick counted");
        assert!(!player.flying);
        // A held jump is not a fresh press, and the window keeps running.
        assert!(!player.update_flight(true));
        assert_eq!(player.fly_toggle_timer, 5);
        assert!(!player.update_flight(false));
        assert_eq!(player.fly_toggle_timer, 4);
        // A second fresh press inside the window flips flight, anywhere: this
        // player is not on the ground.
        assert!(!player.on_ground);
        assert!(player.update_flight(true));
        assert!(player.flying);
        assert!(player.abilities.flying, "the wire copy follows the flip");
        assert_eq!(player.fly_toggle_timer, 0, "the flip closes the window");
        // Held jump again: not fresh, so no second flip.
        assert!(!player.update_flight(true));
        assert!(player.flying);
        // A fresh press with a closed window only arms again.
        assert!(!player.update_flight(false));
        assert!(!player.update_flight(true));
        assert!(player.flying, "the second flip needs a second fresh press");
        // Without allow_flying a fresh press does nothing at all.
        let mut grounded = Player::new();
        assert!(!grounded.update_flight(true));
        assert!(!grounded.flying);
        assert_eq!(grounded.fly_toggle_timer, 0);
    }

    #[test]
    fn the_abilities_flags_byte_is_the_sources_bits() {
        // `C13PacketPlayerAbilities.java:66-71`: 0x01 invulnerable,
        // 0x02 flying, 0x04 allow flying, 0x08 creative.
        let mut abilities = Abilities::default();
        assert_eq!(abilities.flags(), 0x00);
        abilities.allow_flying = true;
        assert_eq!(abilities.flags(), 0x04);
        abilities.flying = true;
        abilities.creative = true;
        assert_eq!(abilities.flags(), 0x0E);
        abilities.invulnerable = true;
        assert_eq!(abilities.flags(), 0x0F);
    }

    #[test]
    fn setting_flight_keeps_both_copies_together() {
        let mut player = Player::new();
        player.set_flying(true);
        assert!(player.flying && player.abilities.flying);
        player.set_flying(false);
        assert!(!player.flying && !player.abilities.flying);
        // A 0x39 replaces every field, and the flying flag follows its own
        // packet value in either direction (`NetHandlerPlayClient.java:1674-1683`).
        let abilities = Abilities {
            allow_flying: true,
            creative: true,
            fly_speed: 0.05,
            walk_speed: 0.1,
            ..Abilities::default()
        };
        player.apply_abilities(abilities);
        assert!(!player.flying, "the packet's flying bit is false here");
        assert!(player.abilities.allow_flying && player.abilities.creative);
        let abilities = Abilities {
            flying: true,
            allow_flying: true,
            ..Abilities::default()
        };
        player.apply_abilities(abilities);
        assert!(player.flying && player.abilities.flying);
    }
}

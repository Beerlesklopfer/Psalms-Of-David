// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Roles and the weekly planner.
//!
//! See `docs/spec-discovery-profiles-roles.md`, sections 3 and 3a.

use std::fmt;
use std::str::FromStr;

/// A fixed set of roles; what each may do is decided in code, their parameters
/// (devices, session lifetime) are data in `role_policy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Admin,
    Member,
    Child,
    Guest,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Child => "child",
            Role::Guest => "guest",
        }
    }

    /// Higher means more privileges.
    fn rank(self) -> u8 {
        match self {
            Role::Admin => 3,
            Role::Member => 2,
            Role::Child => 1,
            Role::Guest => 0,
        }
    }

    /// The less privileged of two roles. A session on a shared device gets the
    /// lower of the user's role and the device owner's role.
    pub fn lower(self, other: Role) -> Role {
        if self.rank() <= other.rank() { self } else { other }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "admin" => Ok(Role::Admin),
            "member" => Ok(Role::Member),
            "child" => Ok(Role::Child),
            "guest" => Ok(Role::Guest),
            other => Err(format!("unknown role: {other}")),
        }
    }
}

pub const SLOT_MINUTES: u16 = 15;
pub const SLOTS_PER_DAY: usize = 96;
pub const SCHEDULE_BYTES: usize = 84;
const WEEK_SLOTS: usize = 7 * SLOTS_PER_DAY;

/// The weekly planner: 7 days of 96 slots of 15 minutes, 672 bits in 84 bytes.
/// Slot number = day * 96 + slot of the day; byte = slot number / 8, bit = slot
/// number % 8, least significant bit first. A set bit means listening is allowed.
/// Day 0 is Monday. Times are wall-clock time of the server's time zone.
#[derive(Clone, PartialEq, Eq)]
pub struct Schedule([u8; SCHEDULE_BYTES]);

impl fmt::Debug for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let allowed = (0..WEEK_SLOTS).filter(|&i| self.bit(i)).count();
        write!(f, "Schedule({allowed} of {WEEK_SLOTS} slots allowed)")
    }
}

impl Schedule {
    /// Nothing is allowed.
    pub fn none() -> Schedule {
        Schedule([0; SCHEDULE_BYTES])
    }

    /// Everything is allowed.
    pub fn always() -> Schedule {
        Schedule([0xff; SCHEDULE_BYTES])
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Schedule> {
        <[u8; SCHEDULE_BYTES]>::try_from(bytes).ok().map(Schedule)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn bit(&self, index: usize) -> bool {
        self.0[index / 8] & (1 << (index % 8)) != 0
    }

    fn set_bit(&mut self, index: usize, on: bool) {
        if on {
            self.0[index / 8] |= 1 << (index % 8);
        } else {
            self.0[index / 8] &= !(1 << (index % 8));
        }
    }

    fn slot_index(day: u8, minute: u16) -> usize {
        (day as usize % 7) * SLOTS_PER_DAY + (minute / SLOT_MINUTES) as usize % SLOTS_PER_DAY
    }

    /// Allows `from_minute..until_minute` of one day (minutes since midnight,
    /// `until` at most 1440). A partial slot at either end is included.
    pub fn allow_range(&mut self, day: u8, from_minute: u16, until_minute: u16) {
        let first = (from_minute / SLOT_MINUTES) as usize;
        let last = (until_minute.min(1440).div_ceil(SLOT_MINUTES)) as usize;
        for slot in first..last.min(SLOTS_PER_DAY) {
            self.set_bit((day as usize % 7) * SLOTS_PER_DAY + slot, true);
        }
    }

    /// Allows the same range on every day.
    pub fn allow_every_day(&mut self, from_minute: u16, until_minute: u16) {
        for day in 0..7 {
            self.allow_range(day, from_minute, until_minute);
        }
    }

    /// Is the slot `slot` (0 to 95, 15 minutes each) of `day` allowed?
    pub fn slot(&self, day: u8, slot: usize) -> bool {
        self.bit((day as usize % 7) * SLOTS_PER_DAY + slot % SLOTS_PER_DAY)
    }

    /// Allows or forbids one slot.
    pub fn set_slot(&mut self, day: u8, slot: usize, allowed: bool) {
        self.set_bit((day as usize % 7) * SLOTS_PER_DAY + slot % SLOTS_PER_DAY, allowed);
    }

    /// Number of allowed slots in the whole week.
    pub fn allowed_slots(&self) -> usize {
        (0..WEEK_SLOTS).filter(|&i| self.bit(i)).count()
    }

    pub fn is_allowed(&self, day: u8, minute: u16) -> bool {
        self.bit(Self::slot_index(day, minute))
    }

    /// The start of the next allowed slot after the one containing the given
    /// time, as `(day, minute of day)`; `None` if nothing is allowed at all.
    pub fn next_start(&self, day: u8, minute: u16) -> Option<(u8, u16)> {
        let now = Self::slot_index(day, minute);
        (1..=WEEK_SLOTS)
            .map(|step| (now + step) % WEEK_SLOTS)
            .find(|&i| self.bit(i))
            .map(|i| ((i / SLOTS_PER_DAY) as u8, ((i % SLOTS_PER_DAY) as u16) * SLOT_MINUTES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_week_fits_in_84_bytes() {
        assert_eq!(WEEK_SLOTS, 672);
        assert_eq!(WEEK_SLOTS / 8, SCHEDULE_BYTES);
    }

    #[test]
    fn a_range_covers_whole_slots() {
        let mut s = Schedule::none();
        s.allow_range(0, 7 * 60, 19 * 60); // Monday 07:00 to 19:00
        assert!(!s.is_allowed(0, 6 * 60 + 59));
        assert!(s.is_allowed(0, 7 * 60));
        assert!(s.is_allowed(0, 18 * 60 + 59));
        assert!(!s.is_allowed(0, 19 * 60));
        assert!(!s.is_allowed(1, 8 * 60), "Tuesday is untouched");
    }

    #[test]
    fn single_slots_can_be_set_and_counted() {
        let mut s = Schedule::none();
        s.set_slot(3, 95, true);
        s.set_slot(0, 0, true);
        assert!(s.slot(3, 95) && s.slot(0, 0) && !s.slot(3, 94));
        assert_eq!(s.allowed_slots(), 2);
        s.set_slot(3, 95, false);
        assert_eq!(s.allowed_slots(), 1);
        assert!(s.is_allowed(0, 14) && !s.is_allowed(0, 15));
    }

    #[test]
    fn a_partial_slot_is_included() {
        let mut s = Schedule::none();
        s.allow_range(2, 7 * 60 + 5, 7 * 60 + 20); // inside two slots
        assert!(s.is_allowed(2, 7 * 60));
        assert!(s.is_allowed(2, 7 * 60 + 15));
        assert!(!s.is_allowed(2, 7 * 60 + 30));
    }

    #[test]
    fn the_end_of_the_day_works() {
        let mut s = Schedule::none();
        s.allow_range(6, 23 * 60, 1440);
        assert!(s.is_allowed(6, 23 * 60 + 45));
        assert!(!s.is_allowed(0, 0));
    }

    #[test]
    fn next_start_finds_the_following_window() {
        let mut s = Schedule::none();
        s.allow_every_day(7 * 60, 19 * 60);
        assert_eq!(s.next_start(0, 20 * 60), Some((1, 7 * 60)));
        assert_eq!(s.next_start(0, 3 * 60), Some((0, 7 * 60)));
        // Sunday evening wraps to Monday morning.
        assert_eq!(s.next_start(6, 20 * 60), Some((0, 7 * 60)));
    }

    #[test]
    fn next_start_of_an_empty_planner_is_none() {
        assert_eq!(Schedule::none().next_start(0, 0), None);
        assert!(Schedule::always().is_allowed(3, 12 * 60));
    }

    #[test]
    fn bytes_round_trip_and_the_length_is_checked() {
        let mut s = Schedule::none();
        s.allow_range(4, 600, 660);
        assert_eq!(Schedule::from_bytes(s.as_bytes()), Some(s));
        assert!(Schedule::from_bytes(&[0; 83]).is_none());
    }

    #[test]
    fn the_lower_role_wins() {
        assert_eq!(Role::Admin.lower(Role::Child), Role::Child);
        assert_eq!(Role::Child.lower(Role::Admin), Role::Child);
        assert_eq!(Role::Member.lower(Role::Member), Role::Member);
        assert_eq!("guest".parse::<Role>(), Ok(Role::Guest));
        assert!("owner".parse::<Role>().is_err());
    }
}

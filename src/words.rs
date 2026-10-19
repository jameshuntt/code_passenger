//! The plan flags as one packed word (big-bad-bool), its rules the word's
//! own and checked by flaga_solver: `check` never writes, so it conflicts
//! with `write`; a write the cargo check did not prove (`write` with
//! `no_cargo`) requires `unproven`, said on purpose; `unproven` means nothing
//! else. The word is what a receipt keeps of a run's flags.

use crate::error::PassengerError;
use big_bad_bool::bitpack;

#[bitpack(storage = u32, version = 1)]
pub struct PlanWord {
    #[bits(4)]
    #[describe("how a features run treats its plan: written or not, checked only, proven by cargo check or written unproven on purpose")]
    #[flags(write = 0, check = 1, no_cargo = 2, unproven = 3)]
    #[labels(write = 1, check = 2, unproven = 8, write_unproven = 5)]
    #[requires(write_unproven => plan.unproven)]
    #[requires(unproven => plan.write_unproven)]
    #[conflicts(write, check)]
    plan: u8,
}

impl PlanWord {
    /// The word from the flags a line carries.
    pub fn of(write: bool, check: bool, no_cargo: bool, unproven: bool) -> PlanWord {
        let mut w = PlanWord::from_raw(0);
        if write {
            w = w.with_plan_write();
        }
        if check {
            w = w.with_plan_check();
        }
        if no_cargo {
            w = w.with_plan_no_cargo();
        }
        if unproven {
            w = w.with_plan_unproven();
        }
        w
    }

    /// The word, or the refusal its rules give: `--write --no-cargo` without
    /// `--unproven` is [`PassengerError::Unproven`]; any other broken rule is
    /// [`PassengerError::PlanFlags`].
    pub fn checked(self) -> std::result::Result<PlanWord, PassengerError> {
        let verdict = flaga_solver::check(&flaga_solver::schema_of!(PlanWord), self.raw() as u128);
        if verdict.is_ok() {
            return Ok(self);
        }
        if self.writes() && self.skips_cargo() && !self.has_plan_unproven() {
            return Err(PassengerError::Unproven);
        }
        Err(PassengerError::PlanFlags { detail: format!("{verdict} (word {:#04x}: {})", self.raw(), self.names().join(", ")) })
    }

    pub fn writes(self) -> bool {
        self.has_plan_write()
    }

    pub fn checks_only(self) -> bool {
        self.has_plan_check()
    }

    pub fn skips_cargo(self) -> bool {
        self.has_plan_no_cargo()
    }

    /// The names of the flags set, in bit order.
    pub fn names(self) -> Vec<&'static str> {
        Self::FLAGS.iter().filter(|f| self.raw() as u128 & f.mask != 0).map(|f| f.name).collect()
    }
}

impl serde::Serialize for PlanWord {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_u32(self.raw())
    }
}

impl<'de> serde::Deserialize<'de> for PlanWord {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = <u32 as serde::Deserialize>::deserialize(d)?;
        PlanWord::try_from_raw(raw).ok_or_else(|| serde::de::Error::custom(format!("{raw:#x} sets bits outside the plan word")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_rules() {
        assert!(PlanWord::of(false, false, false, false).checked().is_ok());
        assert!(PlanWord::of(true, false, false, false).checked().is_ok());
        assert!(PlanWord::of(false, true, true, false).checked().is_ok());
        assert!(PlanWord::of(true, false, true, true).checked().is_ok(), "an unproven write said on purpose");
        assert!(matches!(PlanWord::of(true, false, true, false).checked(), Err(PassengerError::Unproven)));
        assert!(matches!(PlanWord::of(true, true, false, false).checked(), Err(PassengerError::PlanFlags { .. })));
        assert!(matches!(PlanWord::of(false, false, false, true).checked(), Err(PassengerError::PlanFlags { .. })), "--unproven alone means nothing");
        assert_eq!(PlanWord::of(true, false, true, true).names(), vec!["write", "no_cargo", "unproven"]);
    }
}

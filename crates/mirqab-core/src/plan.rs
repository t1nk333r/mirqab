//! Plans for destructive actions (plan 005, "Rules for tools that act";
//! plan 006, part 2). The first call never acts: it makes a plan of the
//! processes it would touch, each a pid with its start time, and returns
//! the plan's id. A second call with `confirm: true` and that id acts only
//! on targets that are both in the plan and there now; a process that
//! appeared since, or a pid reused by another process, is not touched.
//!
//! A plan is bound to its action (`apps.quit`) and its subject (the pid or
//! group asked about), used once, and expires 120 s after it was made on
//! the boot-time clock, so a plan made before a suspend is dead after it.
//! It is not bound to a connection: the CLI and the MCP server connect per
//! call, and the id, keyed at random, is the secret. At most 64 plans are
//! live; a new one drops the oldest.

use crate::act::Target;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

/// How long a plan may wait for its confirmation.
pub const PLAN_TTL: Duration = Duration::from_secs(120);
/// The most plans alive at once: any client of the daemon can make them.
pub const MAX_PLANS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// No such plan: never made, already used, or dropped by the cap.
    Unknown,
    Expired,
    /// The plan was made by another action, or about another subject.
    OtherTool,
}

struct Plan {
    tool: String,
    subject: Value,
    targets: Vec<Target>,
    /// Boot-time clock when it was made.
    made: Duration,
    /// Orders plans made within one clock tick.
    seq: u64,
}

/// The plans the daemon made and not yet used.
pub struct Plans {
    plans: HashMap<String, Plan>,
    /// Keyed at random per process, so ids cannot be guessed from outside.
    keys: RandomState,
    counter: u64,
}

impl Default for Plans {
    fn default() -> Self {
        Plans {
            plans: HashMap::new(),
            keys: RandomState::new(),
            counter: 0,
        }
    }
}

/// The planned targets that are still there, in the plan's order.
pub fn intersect(planned: &[Target], current: &[Target]) -> Vec<Target> {
    planned
        .iter()
        .filter(|t| current.contains(t))
        .copied()
        .collect()
}

/// What a redeemed plan leaves to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redeemed {
    /// Planned and still there.
    pub kept: Vec<Target>,
    /// Planned and not there any more (exited, or the pid reused).
    pub gone: Vec<Target>,
}

impl Plans {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many plans are alive.
    pub fn len(&self) -> usize {
        self.plans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }

    /// Records the targets `tool` would act on for `subject` at `now` (the
    /// boot-time clock) and returns the plan's id.
    pub fn make(
        &mut self,
        tool: &str,
        subject: Value,
        targets: Vec<Target>,
        now: Duration,
    ) -> String {
        self.plans
            .retain(|_, p| now.saturating_sub(p.made) < PLAN_TTL);
        while self.plans.len() >= MAX_PLANS {
            let oldest = self
                .plans
                .iter()
                .min_by_key(|(_, p)| (p.made, p.seq))
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => self.plans.remove(&id),
                None => break,
            };
        }
        self.counter += 1;
        let mut h = self.keys.build_hasher();
        h.write_u64(self.counter);
        h.write(tool.as_bytes());
        let id = format!("plan-{:016x}{:04x}", h.finish(), self.counter & 0xffff);
        self.plans.insert(
            id.clone(),
            Plan {
                tool: tool.to_string(),
                subject,
                targets,
                made: now,
                seq: self.counter,
            },
        );
        id
    }

    /// Uses the plan `id` for `tool` and `subject`: the planned targets
    /// that are in `current` (what the action resolves to now), in the
    /// plan's order, and the planned targets that are not. The plan is gone
    /// afterwards, whatever the result.
    pub fn redeem(
        &mut self,
        id: &str,
        tool: &str,
        subject: &Value,
        current: &[Target],
        now: Duration,
    ) -> Result<Redeemed, PlanError> {
        let plan = self.plans.remove(id).ok_or(PlanError::Unknown)?;
        if plan.tool != tool || plan.subject != *subject {
            return Err(PlanError::OtherTool);
        }
        if now.saturating_sub(plan.made) >= PLAN_TTL {
            return Err(PlanError::Expired);
        }
        let kept = intersect(&plan.targets, current);
        let gone = plan
            .targets
            .into_iter()
            .filter(|t| !kept.contains(t))
            .collect();
        Ok(Redeemed { kept, gone })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(pid: u32, start: u64) -> Target {
        Target { pid, start }
    }

    const T0: Duration = Duration::from_secs(1000);

    #[test]
    fn a_plan_acts_once_on_its_own_targets_that_are_still_there() {
        let mut p = Plans::new();
        let subject = json!({"group": "scope:x"});
        let id = p.make(
            "apps.quit",
            subject.clone(),
            vec![t(101, 5), t(102, 6), t(104, 8)],
            T0,
        );
        // 102 has gone, 103 appeared since, 104's pid was reused: only 101.
        let now = vec![t(101, 5), t(103, 7), t(104, 9)];
        assert_eq!(
            p.redeem(
                &id,
                "apps.quit",
                &subject,
                &now,
                T0 + Duration::from_secs(5)
            ),
            Ok(Redeemed {
                kept: vec![t(101, 5)],
                gone: vec![t(102, 6), t(104, 8)]
            })
        );
        assert_eq!(
            p.redeem(&id, "apps.quit", &subject, &now, T0),
            Err(PlanError::Unknown),
            "used once"
        );
    }

    #[test]
    fn expiry_is_on_the_boot_time_clock() {
        let mut p = Plans::new();
        let s = json!({"pid": 7});
        let id = p.make("apps.quit", s.clone(), vec![t(7, 1)], T0);
        // A suspend of an hour moves the boot-time clock by an hour.
        let after_suspend = T0 + Duration::from_secs(3600);
        assert_eq!(
            p.redeem(&id, "apps.quit", &s, &[t(7, 1)], after_suspend),
            Err(PlanError::Expired)
        );
        let id = p.make("apps.quit", s.clone(), vec![t(7, 1)], T0);
        let just_in_time = T0 + PLAN_TTL - Duration::from_millis(1);
        assert!(
            p.redeem(&id, "apps.quit", &s, &[t(7, 1)], just_in_time)
                .is_ok()
        );
    }

    #[test]
    fn foreign_and_made_up_plans_are_refused() {
        let mut p = Plans::new();
        let s = json!({"pid": 7});
        let id = p.make("apps.quit", s.clone(), vec![t(7, 1)], T0);
        assert_eq!(
            p.redeem(&id, "apps.kill", &s, &[t(7, 1)], T0),
            Err(PlanError::OtherTool)
        );
        let id = p.make("apps.quit", s.clone(), vec![t(7, 1)], T0);
        assert_eq!(
            p.redeem(&id, "apps.quit", &json!({"pid": 8}), &[t(7, 1)], T0),
            Err(PlanError::OtherTool),
            "another subject"
        );
        assert_eq!(
            p.redeem("plan-0000000000000000", "apps.quit", &s, &[], T0),
            Err(PlanError::Unknown)
        );
        let a = p.make("x", Value::Null, vec![], T0);
        let b = p.make("x", Value::Null, vec![], T0);
        assert_ne!(a, b, "ids are unique");
    }

    #[test]
    fn the_cap_drops_the_oldest() {
        let mut p = Plans::new();
        let first = p.make("x", Value::Null, vec![t(1, 1)], T0);
        let second = p.make("x", Value::Null, vec![t(1, 1)], T0);
        for _ in 0..MAX_PLANS - 1 {
            p.make("x", Value::Null, vec![], T0 + Duration::from_secs(1));
        }
        assert_eq!(p.len(), MAX_PLANS);
        assert_eq!(
            p.redeem(&first, "x", &Value::Null, &[t(1, 1)], T0),
            Err(PlanError::Unknown),
            "the oldest went"
        );
        assert!(p.redeem(&second, "x", &Value::Null, &[t(1, 1)], T0).is_ok());
    }

    #[test]
    fn targets_match_on_pid_and_start_time() {
        assert_eq!(
            intersect(&[t(1, 10), t(2, 20), t(3, 30)], &[t(3, 30), t(2, 21)]),
            [t(3, 30)]
        );
    }
}

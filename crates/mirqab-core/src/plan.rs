//! Plans for acting tools (plan 005, "Rules for tools that act"). The first
//! call of an acting tool never acts: it makes a plan of the targets it
//! would touch and returns its id. A second call with `confirm: true` and
//! that id acts only on targets that are both in the plan and still there
//! now; a process that appeared since is not touched. A plan is used once
//! and expires after 120 s.

use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, Instant};

/// How long a plan may wait for its confirmation.
pub const PLAN_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// No such plan: never made, already used, or made by another server.
    Unknown,
    Expired,
    /// The plan was made by another tool.
    OtherTool,
}

struct Plan {
    tool: String,
    targets: Vec<Value>,
    made: Instant,
}

/// The plans one MCP server made.
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

impl Plans {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the targets `tool` would act on and returns the plan's id.
    pub fn make(&mut self, tool: &str, targets: Vec<Value>, now: Instant) -> String {
        self.plans
            .retain(|_, p| now.saturating_duration_since(p.made) < PLAN_TTL);
        self.counter += 1;
        let mut h = self.keys.build_hasher();
        h.write_u64(self.counter);
        h.write(tool.as_bytes());
        let id = format!("plan-{:016x}{:04x}", h.finish(), self.counter & 0xffff);
        self.plans.insert(
            id.clone(),
            Plan {
                tool: tool.to_string(),
                targets,
                made: now,
            },
        );
        id
    }

    /// Uses the plan `id` for `tool`: the targets that were planned and
    /// are in `current` (what the tool sees now), in the plan's order. The
    /// plan is gone afterwards, whatever the result.
    pub fn redeem(
        &mut self,
        id: &str,
        tool: &str,
        current: &[Value],
        now: Instant,
    ) -> Result<Vec<Value>, PlanError> {
        let plan = self.plans.remove(id).ok_or(PlanError::Unknown)?;
        if plan.tool != tool {
            return Err(PlanError::OtherTool);
        }
        if now.saturating_duration_since(plan.made) >= PLAN_TTL {
            return Err(PlanError::Expired);
        }
        Ok(plan
            .targets
            .into_iter()
            .filter(|t| current.contains(t))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_plan_acts_once_on_its_own_targets_that_are_still_there() {
        let mut p = Plans::new();
        let t0 = Instant::now();
        let id = p.make("kill_port", vec![json!(101), json!(102)], t0);
        // 102 has gone, 103 appeared since: only 101.
        let now = vec![json!(101), json!(103)];
        assert_eq!(
            p.redeem(&id, "kill_port", &now, t0 + Duration::from_secs(5)),
            Ok(vec![json!(101)])
        );
        assert_eq!(
            p.redeem(&id, "kill_port", &now, t0 + Duration::from_secs(6)),
            Err(PlanError::Unknown),
            "used once"
        );
    }

    #[test]
    fn expired_foreign_and_made_up_plans_are_refused() {
        let mut p = Plans::new();
        let t0 = Instant::now();
        let id = p.make("kill_port", vec![json!(7)], t0);
        assert_eq!(
            p.redeem(&id, "kill_port", &[json!(7)], t0 + PLAN_TTL),
            Err(PlanError::Expired)
        );
        let id = p.make("kill_port", vec![json!(7)], t0);
        assert_eq!(
            p.redeem(&id, "cleanup_agent_leftovers", &[json!(7)], t0),
            Err(PlanError::OtherTool)
        );
        assert_eq!(
            p.redeem("plan-0000000000000000", "kill_port", &[json!(7)], t0),
            Err(PlanError::Unknown)
        );
        let a = p.make("x", vec![], t0);
        let b = p.make("x", vec![], t0);
        assert_ne!(a, b, "ids are unique");
    }
}

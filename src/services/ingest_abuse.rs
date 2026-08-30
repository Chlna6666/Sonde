#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RiskTier {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IngestAbusePolicy {
    pub tier: RiskTier,
    pub token_ttl_seconds: i64,
    pub token_cost_multiplier: u64,
    pub request_cost_multiplier: u64,
    pub byte_cost_multiplier: u64,
    pub item_cost_multiplier: u64,
}

const INTERNAL_RISK_SCOPE_PREFIX: &str = "sonde.internal.risk.";

pub(crate) fn policy_for_score(score: i32) -> IngestAbusePolicy {
    policy_for_tier(tier_for_score(score))
}

pub(crate) fn policy_for_claims(scopes: &[String]) -> IngestAbusePolicy {
    policy_for_tier(tier_from_claim_scopes(scopes))
}

pub(crate) fn policy_for_tier(tier: RiskTier) -> IngestAbusePolicy {
    match tier {
        RiskTier::Low => IngestAbusePolicy {
            tier,
            token_ttl_seconds: 120,
            token_cost_multiplier: 1,
            request_cost_multiplier: 1,
            byte_cost_multiplier: 1,
            item_cost_multiplier: 1,
        },
        RiskTier::Medium => IngestAbusePolicy {
            tier,
            token_ttl_seconds: 90,
            token_cost_multiplier: 1,
            request_cost_multiplier: 2,
            byte_cost_multiplier: 2,
            item_cost_multiplier: 2,
        },
        RiskTier::High => IngestAbusePolicy {
            tier,
            token_ttl_seconds: 60,
            token_cost_multiplier: 2,
            request_cost_multiplier: 4,
            byte_cost_multiplier: 4,
            item_cost_multiplier: 4,
        },
        RiskTier::Critical => IngestAbusePolicy {
            tier,
            token_ttl_seconds: 45,
            token_cost_multiplier: 4,
            request_cost_multiplier: 8,
            byte_cost_multiplier: 8,
            item_cost_multiplier: 8,
        },
    }
}

pub(crate) fn claims_scopes(scopes: &[String], tier: RiskTier) -> Vec<String> {
    let mut result = scopes
        .iter()
        .filter(|scope| !scope.starts_with(INTERNAL_RISK_SCOPE_PREFIX))
        .cloned()
        .collect::<Vec<_>>();
    result.push(format!("{INTERNAL_RISK_SCOPE_PREFIX}{}", tier_name(tier)));
    result
}

pub(crate) fn scaled_cost(value: usize, multiplier: u64) -> usize {
    value.saturating_mul(usize::try_from(multiplier).unwrap_or(usize::MAX))
}

fn tier_for_score(score: i32) -> RiskTier {
    match score {
        80.. => RiskTier::Critical,
        50..=79 => RiskTier::High,
        20..=49 => RiskTier::Medium,
        _ => RiskTier::Low,
    }
}

fn tier_from_claim_scopes(scopes: &[String]) -> RiskTier {
    scopes.iter().fold(RiskTier::Low, |current, scope| {
        let tier = match scope.strip_prefix(INTERNAL_RISK_SCOPE_PREFIX) {
            Some("medium") => RiskTier::Medium,
            Some("high") => RiskTier::High,
            Some("critical") => RiskTier::Critical,
            _ => RiskTier::Low,
        };
        current.max(tier)
    })
}

fn tier_name(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::Low => "low",
        RiskTier::Medium => "medium",
        RiskTier::High => "high",
        RiskTier::Critical => "critical",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RiskTier, claims_scopes, policy_for_claims, policy_for_score, scaled_cost,
    };

    #[test]
    fn risk_tiers_reduce_ingest_budget_without_blocking() {
        let low = policy_for_score(0);
        let medium = policy_for_score(20);
        let high = policy_for_score(50);
        let critical = policy_for_score(80);
        assert_eq!(low.tier, RiskTier::Low);
        assert_eq!(medium.tier, RiskTier::Medium);
        assert_eq!(high.tier, RiskTier::High);
        assert_eq!(critical.tier, RiskTier::Critical);
        assert!(critical.token_ttl_seconds < low.token_ttl_seconds);
        assert!(critical.item_cost_multiplier > low.item_cost_multiplier);
    }

    #[test]
    fn server_risk_marker_cannot_be_downgraded_by_existing_scopes() {
        let original = vec![
            "telemetry.ingest".to_string(),
            "sonde.internal.risk.low".to_string(),
        ];
        let claims = claims_scopes(&original, RiskTier::High);
        assert_eq!(policy_for_claims(&claims).tier, RiskTier::High);
        assert_eq!(
            claims
                .iter()
                .filter(|scope| scope.starts_with("sonde.internal.risk."))
                .count(),
            1
        );
    }

    #[test]
    fn cost_scaling_saturates() {
        assert_eq!(scaled_cost(100, 4), 400);
        assert_eq!(scaled_cost(usize::MAX, 8), usize::MAX);
    }
}

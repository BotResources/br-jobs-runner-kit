use br_runner_sandbox::Timestamp;

use crate::convert::timestamp_of;
use crate::resource::SandboxPool;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PoolRef {
    pub name: String,
    pub created_at: Option<Timestamp>,
}

impl PoolRef {
    pub fn new(name: impl Into<String>, created_at: Option<Timestamp>) -> Self {
        Self {
            name: name.into(),
            created_at,
        }
    }

    pub fn of(pool: &SandboxPool) -> Self {
        Self {
            name: pool.metadata.name.clone().unwrap_or_default(),
            created_at: pool.metadata.creation_timestamp.as_ref().map(timestamp_of),
        }
    }

    fn rank(&self) -> (bool, Option<Timestamp>, &str) {
        (self.created_at.is_none(), self.created_at, &self.name)
    }
}

pub fn pool_winner(pools: &[PoolRef]) -> Option<&PoolRef> {
    pools.iter().min_by(|a, b| a.rank().cmp(&b.rank()))
}

pub fn is_duplicate(pool: &PoolRef, pools_of_the_namespace: &[PoolRef]) -> bool {
    pool_winner(pools_of_the_namespace).is_some_and(|winner| winner.name != pool.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::at;

    fn pool(name: &str, created: Option<u64>) -> PoolRef {
        PoolRef::new(name, created.map(at))
    }

    #[test]
    fn a_namespace_with_one_resource_has_no_duplicate() {
        let only = pool("a", Some(10));
        assert!(!is_duplicate(&only, std::slice::from_ref(&only)));
        assert_eq!(pool_winner(&[]), None);
    }

    #[test]
    fn the_older_resource_wins_and_the_newer_is_a_duplicate() {
        let older = pool("zzz", Some(10));
        let newer = pool("aaa", Some(20));
        let both = [newer.clone(), older.clone()];
        assert_eq!(pool_winner(&both), Some(&older));
        assert!(!is_duplicate(&older, &both));
        assert!(is_duplicate(&newer, &both));
    }

    #[test]
    fn at_the_same_creation_time_the_smaller_name_wins() {
        let a = pool("alpha", Some(10));
        let b = pool("beta", Some(10));
        let both = [b.clone(), a.clone()];
        assert!(!is_duplicate(&a, &both));
        assert!(is_duplicate(&b, &both));
    }

    #[test]
    fn a_resource_with_no_creation_time_never_wins_over_one_that_has_it() {
        let dated = pool("zzz", Some(10));
        let undated = pool("aaa", None);
        let both = [undated.clone(), dated.clone()];
        assert!(is_duplicate(&undated, &both));
        assert!(!is_duplicate(&dated, &both));
    }

    #[test]
    fn when_the_winner_goes_the_other_resource_is_judged_again_and_wins() {
        let older = pool("old", Some(10));
        let newer = pool("new", Some(20));
        assert!(is_duplicate(&newer, &[older, newer.clone()]));
        assert!(!is_duplicate(&newer, std::slice::from_ref(&newer)));
    }

    #[test]
    fn three_resources_leave_one_winner_and_two_duplicates() {
        let all = [
            pool("c", Some(30)),
            pool("a", Some(10)),
            pool("b", Some(20)),
        ];
        let duplicates = all.iter().filter(|pool| is_duplicate(pool, &all)).count();
        assert_eq!(duplicates, 2);
        assert_eq!(pool_winner(&all).unwrap().name, "a");
    }
}

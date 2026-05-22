//! Centralized GraphQL query strings.
//!
//! Cadenza only issues read-only queries against Linear. The shape of
//! each query is owned here so the orchestrator never assembles GraphQL
//! directly — that boundary is a hard rule per `ARCHITECTURE.md`.

/// Active project issues that the orchestrator might dispatch.
pub const CANDIDATE_ISSUES: &str = r#"
query CandidateIssues($projectId: String!, $first: Int!, $after: String) {
  issues(
    filter: { project: { id: { eq: $projectId } } }
    first: $first
    after: $after
    orderBy: updatedAt
  ) {
    nodes {
      id
      identifier
      title
      description
      priority
      state { name }
      branchName
      url
      labels(first: 32) { nodes { name } }
      createdAt
      updatedAt
      inverseRelations(first: 32) {
        nodes {
          issue { id identifier state { name } }
        }
      }
    }
    pageInfo { hasNextPage endCursor }
  }
}
"#;

/// Issues in the configured project whose `state.name` is in the
/// provided set. The project filter is mandatory — otherwise a
/// workspace with multiple projects sharing state names would mix
/// foreign issues into the orchestrator's view.
pub const ISSUES_BY_STATES: &str = r#"
query IssuesByStates(
  $projectId: String!
  $states: [String!]!
  $first: Int!
  $after: String
) {
  issues(
    filter: {
      project: { id: { eq: $projectId } }
      state: { name: { in: $states } }
    }
    first: $first
    after: $after
    orderBy: updatedAt
  ) {
    nodes {
      id
      identifier
      title
      description
      priority
      state { name }
      branchName
      url
      labels(first: 32) { nodes { name } }
      createdAt
      updatedAt
      inverseRelations(first: 32) {
        nodes {
          issue { id identifier state { name } }
        }
      }
    }
    pageInfo { hasNextPage endCursor }
  }
}
"#;

/// Lookup the current state name for each issue id. Paginated so id
/// sets larger than Linear's default connection page size do not get
/// silently truncated.
pub const ISSUE_STATES_BY_IDS: &str = r#"
query IssueStatesByIds($ids: [String!]!, $first: Int!, $after: String) {
  issues(filter: { id: { in: $ids } }, first: $first, after: $after) {
    nodes {
      id
      state { name }
    }
    pageInfo { hasNextPage endCursor }
  }
}
"#;

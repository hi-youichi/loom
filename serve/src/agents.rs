//! Handle `AgentList` requests.

use loom::{
    list_available_profiles, AgentListRequest, AgentListResponse, AgentSource, AgentSourceFilter,
    AgentSummary, ProfileSource, ServerResponse,
};

pub(crate) async fn handle_agent_list(r: AgentListRequest) -> ServerResponse {
    let id = r.id.clone();

    let all_profiles = list_available_profiles();

    // Filter by source if requested
    let filtered_profiles: Vec<_> = if let Some(filter) = &r.source_filter {
        all_profiles
            .into_iter()
            .filter(|p| match (&p.source, filter) {
                (ProfileSource::BuiltIn, AgentSourceFilter::BuiltIn) => true,
                (ProfileSource::Project, AgentSourceFilter::Project) => true,
                (ProfileSource::User, AgentSourceFilter::User) => true,
                _ if *filter == AgentSourceFilter::BuiltIn
                    && p.source == ProfileSource::BuiltIn =>
                {
                    true
                }
                _ if *filter == AgentSourceFilter::Project
                    && p.source == ProfileSource::Project =>
                {
                    true
                }
                _ if *filter == AgentSourceFilter::User && p.source == ProfileSource::User => true,
                _ => false,
            })
            .collect()
    } else {
        all_profiles
    };

    // Convert to protocol types
    let agents: Vec<AgentSummary> = filtered_profiles
        .into_iter()
        .map(|p| {
            let source = match p.source {
                ProfileSource::BuiltIn => AgentSource::BuiltIn,
                ProfileSource::Project => AgentSource::Project,
                ProfileSource::User => AgentSource::User,
            };
            AgentSummary {
                name: p.name,
                description: p.description,
                source,
            }
        })
        .collect();

    ServerResponse::AgentList(AgentListResponse { id, agents })
}

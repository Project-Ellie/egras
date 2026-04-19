use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::OpenApi;

// TODO: version is hardcoded — utoipa 4's info(version = ...) requires a string literal.
// Revisit when we upgrade utoipa or add build-time spec post-processing.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "egras API",
        version = "0.1.0",
        description = "Enterprise-ready Rust application seed — tenants & audit",
    ),
    paths(
        crate::tenants::interface::post_create_organisation,
        crate::tenants::interface::get_list_my_organisations,
        crate::tenants::interface::get_list_members,
        crate::tenants::interface::post_assign_role,
    ),
    components(
        schemas(
            crate::tenants::interface::CreateOrganisationRequest,
            crate::tenants::interface::OrganisationBody,
            crate::tenants::interface::PagedOrganisations,
            crate::tenants::interface::MemberBody,
            crate::tenants::interface::PagedMembers,
            crate::tenants::interface::AssignRoleRequest,
            crate::tenants::interface::AssignRoleResponseBody,
            crate::errors::ErrorBody,
        ),
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "tenants", description = "Organisation and role management"),
    ),
)]
pub struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .as_mut()
            .expect("components always set by derive");
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

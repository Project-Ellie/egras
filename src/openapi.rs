use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "egras API",
        version = "0.1.0",
        description = "Enterprise-ready Rust application seed — tenants, security & audit",
    ),
    paths(
        crate::tenants::interface::post_create_organisation,
        crate::tenants::interface::get_list_my_organisations,
        crate::tenants::interface::get_list_members,
        crate::tenants::interface::post_assign_role,
        crate::tenants::interface::post_add_user_to_organisation,
        crate::tenants::interface::post_remove_user_from_organisation,
        crate::security::interface::post_register,
        crate::security::interface::post_login,
        crate::security::interface::post_logout,
        crate::security::interface::post_change_password,
        crate::security::interface::post_switch_org,
        crate::security::interface::post_password_reset_request,
        crate::security::interface::post_password_reset_confirm,
        crate::security::interface::get_list_users,
        crate::tenants::interface::post_create_channel,
        crate::tenants::interface::get_list_channels,
        crate::tenants::interface::get_channel,
        crate::tenants::interface::put_update_channel,
        crate::tenants::interface::delete_channel,
    ),
    components(
        schemas(
            crate::tenants::interface::CreateChannelRequest,
            crate::tenants::interface::UpdateChannelRequest,
            crate::tenants::interface::ChannelBody,
            crate::tenants::interface::PagedChannels,
            crate::tenants::model::ChannelType,
            crate::tenants::interface::CreateOrganisationRequest,
            crate::tenants::interface::OrganisationBody,
            crate::tenants::interface::PagedOrganisations,
            crate::tenants::interface::MemberBody,
            crate::tenants::interface::PagedMembers,
            crate::tenants::interface::AssignRoleRequest,
            crate::tenants::interface::AssignRoleResponseBody,
            crate::tenants::interface::AddUserToOrganisationRequest,
            crate::tenants::interface::RemoveUserFromOrganisationRequest,
            crate::security::interface::RegisterRequest,
            crate::security::interface::RegisterResponse,
            crate::security::interface::LoginRequest,
            crate::security::interface::LoginResponse,
            crate::security::interface::MembershipDto,
            crate::security::interface::ChangePasswordRequest,
            crate::security::interface::SwitchOrgRequest,
            crate::security::interface::TokenResponse,
            crate::security::interface::PasswordResetRequestBody,
            crate::security::interface::PasswordResetConfirmBody,
            crate::security::interface::UserSummaryDto,
            crate::security::interface::ListUsersResponse,
            crate::errors::ErrorBody,
        ),
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "tenants", description = "Organisation and role management"),
        (name = "security", description = "Authentication and user management"),
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

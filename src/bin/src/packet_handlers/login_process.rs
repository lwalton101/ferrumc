use ferrumc_macros::event_handler;
use ferrumc_net::errors::NetError;
use ferrumc_net::packets::incoming::login_start::LoginStartEvent;
use ferrumc_net::GlobalState;
use tracing::{info, trace};
use ferrumc_net::connection::{StreamWriter};
use ferrumc_net::packets::outgoing::login_success::LoginSuccessPacket;
use ferrumc_net_codec::encode::NetEncodeOpts;

#[event_handler]
async fn handle_login_start(
    login_start_event: LoginStartEvent,
    _state: GlobalState,
) -> Result<LoginStartEvent, NetError> {

    info!("Handling login start event");

    let uuid = login_start_event.login_start_packet.uuid;
    let username = login_start_event.login_start_packet.username.clone();
    trace!("Received login start from user with username {}", username);

    //Send a Login Success Response to further the login sequence
    let response = LoginSuccessPacket::new(uuid, username);
    let mut writer = _state
        .universe
        .get_mut::<StreamWriter>(login_start_event.conn_id)?;

    writer.send_packet(&response, &NetEncodeOpts::WithLength).await?;
    Ok(login_start_event)
}

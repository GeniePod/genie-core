use crate::ha::{HomeAction, HomeActionKind, HomeAutomationProvider};
use anyhow::Result;

/// Execute a structured home control action via the HA provider.
pub async fn control(
    home: &dyn HomeAutomationProvider,
    target_query: &str,
    action: &str,
    value: Option<f64>,
) -> Result<String> {
    let action_kind = parse_action(action)?;
    let target = home.resolve_target(target_query, Some(action_kind)).await?;
    let result = home
        .execute(HomeAction {
            kind: action_kind,
            target,
            value,
        })
        .await?;
    Ok(result.spoken_summary)
}

/// Query entity or room status via the HA provider.
pub async fn status(home: &dyn HomeAutomationProvider, target_query: &str) -> Result<String> {
    let target = home.resolve_target(target_query, None).await?;
    let state = home.get_state(&target).await?;
    Ok(state.spoken_summary)
}

fn parse_action(action: &str) -> Result<HomeActionKind> {
    let parsed = match action {
        "turn_on" => HomeActionKind::TurnOn,
        "turn_off" => HomeActionKind::TurnOff,
        "toggle" => HomeActionKind::Toggle,
        "set_brightness" => HomeActionKind::SetBrightness,
        "set_temperature" => HomeActionKind::SetTemperature,
        "open" => HomeActionKind::Open,
        "close" => HomeActionKind::Close,
        "lock" => HomeActionKind::Lock,
        "unlock" => HomeActionKind::Unlock,
        "activate" | "activate_scene" => HomeActionKind::Activate,
        other => anyhow::bail!("unknown home action: {}", other),
    };
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_activate_alias() {
        assert_eq!(
            parse_action("activate_scene").unwrap(),
            HomeActionKind::Activate
        );
    }

    #[test]
    fn parse_open_and_close() {
        assert_eq!(parse_action("open").unwrap(), HomeActionKind::Open);
        assert_eq!(parse_action("close").unwrap(), HomeActionKind::Close);
    }
}

use crate::ha::HaClient;
use anyhow::Result;

/// Execute a home control action via HA.
pub async fn control(
    ha: &HaClient,
    entity_name: &str,
    action: &str,
    value: Option<f64>,
) -> Result<String> {
    let entity = ha
        .find_entity(entity_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no device found matching '{}'", entity_name))?;

    let entity_id = &entity.entity_id;
    let domain = entity_id.split('.').next().unwrap_or("light");

    let (service, data) = match action {
        "turn_on" => {
            let mut d = serde_json::json!({"entity_id": entity_id});
            if let Some(v) = value
                && domain == "light"
            {
                d["brightness"] = serde_json::json!(v as u8);
            }
            ("turn_on", d)
        }
        "turn_off" => ("turn_off", serde_json::json!({"entity_id": entity_id})),
        "toggle" => ("toggle", serde_json::json!({"entity_id": entity_id})),
        "set_brightness" => {
            let brightness = value.unwrap_or(128.0) as u8;
            (
                "turn_on",
                serde_json::json!({"entity_id": entity_id, "brightness": brightness}),
            )
        }
        "set_temperature" => {
            let temp = value.unwrap_or(20.0);
            (
                "set_temperature",
                serde_json::json!({"entity_id": entity_id, "temperature": temp}),
            )
        }
        "lock" => ("lock", serde_json::json!({"entity_id": entity_id})),
        "unlock" => ("unlock", serde_json::json!({"entity_id": entity_id})),
        other => return Err(anyhow::anyhow!("unknown action: {}", other)),
    };

    ha.call_service(domain, service, &data).await?;

    Ok(format!(
        "Done. {} {} ({}).",
        action.replace('_', " "),
        entity.friendly_name(),
        entity_id
    ))
}

/// Query entity status via HA.
pub async fn status(ha: &HaClient, entity_name: &str) -> Result<String> {
    let entity = ha
        .find_entity(entity_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no device found matching '{}'", entity_name))?;

    let name = entity.friendly_name().to_string();
    let state = &entity.state;

    let detail = match entity.entity_id.split('.').next().unwrap_or("") {
        "light" => {
            let brightness = entity
                .attributes
                .get("brightness")
                .and_then(|v| v.as_u64())
                .map(|b| format!(", brightness {}%", b * 100 / 255))
                .unwrap_or_default();
            format!("{} is {}{}", name, state, brightness)
        }
        "climate" => {
            let temp = entity
                .attributes
                .get("current_temperature")
                .and_then(|v| v.as_f64())
                .map(|t| format!(", current {}°", t))
                .unwrap_or_default();
            let target = entity
                .attributes
                .get("temperature")
                .and_then(|v| v.as_f64())
                .map(|t| format!(", target {}°", t))
                .unwrap_or_default();
            format!("{} is {}{}{}", name, state, temp, target)
        }
        _ => format!("{} is {}", name, state),
    };

    Ok(detail)
}

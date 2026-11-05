//! Secours vectoriel IGN BD TOPO sous Licence Ouverte 2.0 (pas de tuiles raster).
use crate::maps::{client, limited, point, Feature, Plan};
use anyhow::{bail, Context, Result};
use serde_json::Value;
pub async fn fetch(endpoint: &str, extent: [f64; 4]) -> Result<Plan> {
    let bbox = format!(
        "{},{},{},{},CRS:84",
        extent[0], extent[1], extent[2], extent[3]
    );
    async fn layer(endpoint: &str, bbox: &str, table: &str, properties: &str) -> Result<Value> {
        let response = client()?
            .get(endpoint)
            .timeout(std::time::Duration::from_secs(12))
            .query(&[
                ("SERVICE", "WFS"),
                ("VERSION", "2.0.0"),
                ("REQUEST", "GetFeature"),
                ("TYPENAMES", table),
                ("SRSNAME", "CRS:84"),
                ("BBOX", bbox),
                ("OUTPUTFORMAT", "application/json"),
                ("COUNT", "1000"),
                ("PROPERTYNAME", properties),
            ])
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!("Le service IGN est injoignable ou ne répond pas à temps.")
            })?;
        Ok(
            serde_json::from_slice(&limited(response, 3 * 1024 * 1024).await?)
                .context("Réponse IGN non reconnue")?,
        )
    }
    let (roads, buildings) = tokio::try_join!(
        layer(
            endpoint,
            &bbox,
            "BDTOPO_V3:troncon_de_route",
            "geometrie,nom_collaboratif_gauche,nom_collaboratif_droite"
        ),
        layer(endpoint, &bbox, "BDTOPO_V3:batiment", "geometrie")
    )?;
    parse([roads, buildings], extent)
}
pub fn parse(layers: [Value; 2], extent: [f64; 4]) -> Result<Plan> {
    let mut features = vec![];
    let mut count = 0usize;
    let mut truncated = false;
    for (i, layer) in layers.iter().enumerate() {
        let rows = layer["features"].as_array().context("Objets IGN absents")?;
        if rows.len() >= 1000 {
            truncated = true
        }
        for row in rows {
            let g = &row["geometry"];
            if g.is_null() {
                continue;
            }
            let coords = &g["coordinates"];
            let kind = g["type"].as_str().context("Géométrie IGN absente")?;
            let parts: Vec<&Value> = match kind {
                "LineString" => vec![coords],
                "MultiLineString" => coords
                    .as_array()
                    .context("Lignes IGN invalides")?
                    .iter()
                    .collect(),
                "Polygon" => coords
                    .as_array()
                    .and_then(|p| p.first())
                    .into_iter()
                    .collect(),
                "MultiPolygon" => coords
                    .as_array()
                    .context("Polygones IGN invalides")?
                    .iter()
                    .filter_map(|p| p.as_array().and_then(|a| a.first()))
                    .collect(),
                _ => continue,
            };
            let name = [
                "nom_collaboratif_gauche",
                "nom_collaboratif_droite",
                "nom_voie_ban_gauche",
                "nom_voie_ban_droite",
            ]
            .iter()
            .find_map(|key| {
                row["properties"][*key]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or("")
            .chars()
            .take(250)
            .collect::<String>();
            for part in parts {
                let points = part.as_array().context("Coordonnées IGN invalides")?;
                if points.len() < 2 {
                    continue;
                }
                if features.len() >= 2000 || count + points.len() > 50000 {
                    truncated = true;
                    break;
                }
                let mut geometry = vec![];
                for p in points {
                    let lon = p[0].as_f64().context("Longitude IGN invalide")?;
                    let lat = p[1].as_f64().context("Latitude IGN invalide")?;
                    point(lat, lon)?;
                    geometry.push([lon, lat]);
                }
                count += geometry.len();
                features.push(Feature {
                    kind: if i == 0 { "road" } else { "building" }.into(),
                    road_class: None,
                    name: name.clone(),
                    points: geometry,
                    holes: vec![],
                });
            }
        }
    }
    if features.is_empty() {
        bail!("Aucun fond de plan disponible à cette position. Vous pouvez conserver les coordonnées seules.")
    }
    Ok(Plan {
        bounds: extent,
        features,
        obtained_at: crate::db::now(),
        truncated,
        source: "ign".into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn ign_coordinates_are_longitude_latitude_and_height_is_ignored() {
        let p=parse([json!({"features":[{"geometry":{"type":"LineString","coordinates":[[2.35,48.85,40],[2.351,48.851,41]]},"properties":{"nom_collaboratif_gauche":"Rue exemple"}}]}),json!({"features":[{"geometry":{"type":"MultiPolygon","coordinates":[[[[2.35,48.85,40],[2.351,48.85,40],[2.35,48.85,40]]]]}}]})],crate::maps::bounds(48.85,2.35)).unwrap();
        assert_eq!(p.source, "ign");
        assert_eq!(p.features[0].points[0], [2.35, 48.85]);
        assert_eq!(p.features[0].name, "Rue exemple");
        assert_eq!(p.features[1].kind, "building");
        assert!(crate::maps::validate(&Some(crate::maps::Location {
            lat: 48.85,
            lon: 2.35,
            source_address: String::new(),
            plan: Some(p)
        }))
        .is_ok());
    }
    #[test]
    fn old_osm_plans_keep_their_attribution_and_unknown_sources_are_rejected() {
        let b = crate::maps::bounds(48.85, 2.35);
        let mut v = serde_json::json!({"bounds":b,"features":[],"obtained_at":"2026-10-29T10:00:00Z","truncated":false});
        let plan: Plan = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(plan.source, "osm");
        v["source"] = serde_json::json!("unknown");
        let plan: Plan = serde_json::from_value(v).unwrap();
        assert!(crate::maps::validate(&Some(crate::maps::Location {
            lat: 48.85,
            lon: 2.35,
            source_address: String::new(),
            plan: Some(plan)
        }))
        .is_err());
    }
}

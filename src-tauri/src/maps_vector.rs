//! Fond OSM vectoriel statique (OpenMapTiles), sans requête Overpass.
//! Petites emprises à la demande, au plus 4 tuiles / 12 Mio par plan ; cache chiffré.
//! Schéma MVT 2.1 : https://github.com/mapbox/vector-tile-spec/tree/master/2.1
use crate::{
    crypto, db,
    maps::{self, Feature, Plan},
    Core,
};
use anyhow::{bail, Context, Result};
use prost::Message;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::PathBuf,
    time::{Duration, SystemTime},
};
const MAX_TILE: usize = 3 * 1024 * 1024;
const MAX_DECODED: usize = 8 * 1024 * 1024;
const CACHE_AAD: &[u8] = b"LibreERP-map-cache-v1";
pub const ENDPOINT: &str = "https://tiles.openfreemap.org/planet";

#[derive(Clone, PartialEq, Message)]
struct Tile {
    #[prost(message, repeated, tag = "3")]
    layers: Vec<Layer>,
}
#[derive(Clone, PartialEq, Message)]
struct Layer {
    #[prost(uint32, required, tag = "15")]
    version: u32,
    #[prost(string, required, tag = "1")]
    name: String,
    #[prost(message, repeated, tag = "2")]
    features: Vec<TileFeature>,
    #[prost(string, repeated, tag = "3")]
    keys: Vec<String>,
    #[prost(message, repeated, tag = "4")]
    values: Vec<TileValue>,
    #[prost(uint32, optional, tag = "5")]
    extent: Option<u32>,
}
#[derive(Clone, PartialEq, Message)]
struct TileFeature {
    #[prost(uint32, repeated, packed = "true", tag = "2")]
    tags: Vec<u32>,
    #[prost(uint32, optional, tag = "3")]
    kind: Option<u32>,
    #[prost(uint32, repeated, packed = "true", tag = "4")]
    geometry: Vec<u32>,
}
#[derive(Clone, PartialEq, Message)]
struct TileValue {
    #[prost(string, optional, tag = "1")]
    text: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TileId {
    z: u32,
    x: u32,
    y: u32,
}
fn tile_xy(lon: f64, lat: f64, z: u32) -> (f64, f64) {
    let n = (1u32 << z) as f64;
    (
        (lon + 180.) / 360. * n,
        (1. - lat.to_radians().tan().asinh() / std::f64::consts::PI) / 2. * n,
    )
}
fn tile_ids(b: [f64; 4], max_zoom: u32) -> Result<Vec<TileId>> {
    for z in (0..=max_zoom.min(14)).rev() {
        let n = 1u32 << z;
        let (w, north) = tile_xy(b[0], b[3], z);
        let (e, south) = tile_xy(b[2], b[1], z);
        let cell = |v: f64| v.floor().clamp(0., (n - 1) as f64) as u32;
        let (x0, x1, y0, y1) = (cell(w), cell(e - 1e-10), cell(north), cell(south - 1e-10));
        if (x1 - x0 + 1) as u64 * (y1 - y0 + 1) as u64 <= 4 {
            return Ok((x0..=x1)
                .flat_map(|x| (y0..=y1).map(move |y| TileId { z, x, y }))
                .collect());
        }
    }
    bail!("Emprise du plan vectoriel trop grande")
}
fn tile_url(template: &str, endpoint: &str, t: TileId) -> Result<String> {
    for placeholder in ["{z}", "{x}", "{y}"] {
        if template.matches(placeholder).count() != 1 {
            bail!("Modèle de tuile vectorielle invalide");
        }
    }
    let text = template
        .replace("{z}", &t.z.to_string())
        .replace("{x}", &t.x.to_string())
        .replace("{y}", &t.y.to_string());
    maps::endpoint(&text)?;
    let url = url::Url::parse(&text)?;
    let origin = url::Url::parse(endpoint)?;
    if url.host_str() != origin.host_str()
        || url.port_or_known_default() != origin.port_or_known_default()
        || text.contains(['{', '}'])
    {
        bail!("Le serveur de tuiles doit appartenir au service configuré");
    }
    Ok(text)
}
fn cache_path(c: &Core, url: &str) -> PathBuf {
    c.root.join("map-cache").join(format!(
        "{}.bin",
        hex::encode(Sha256::digest(url.as_bytes()))
    ))
}
fn cached(c: &Core, url: &str, max: usize) -> Option<Vec<u8>> {
    let path = cache_path(c, url);
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() > max as u64 + 28
        || SystemTime::now()
            .duration_since(meta.modified().ok()?)
            .ok()?
            > Duration::from_secs(7 * 86400)
    {
        return None;
    }
    let bytes = crypto::unseal(&c.key, &std::fs::read(path).ok()?, CACHE_AAD).ok()?;
    Some(bytes.to_vec())
}
fn store(c: &Core, url: &str, bytes: &[u8]) -> Result<()> {
    let dir = c.root.join("map-cache");
    std::fs::create_dir_all(&dir)?;
    let dir = dir.canonicalize()?;
    let base = c.root.canonicalize()?;
    if dir == base || !dir.starts_with(&base) {
        bail!("Le cache cartographique sort du dossier local autorisé");
    }
    db::atomic_write(
        &cache_path(c, url),
        &crypto::seal(&c.key, bytes, CACHE_AAD)?,
    )?;
    // Purge uniquement des entrées de ce cache, jamais des documents ou de la base.
    let mut entries = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.len() == 68
                && n.ends_with(".bin")
                && n.as_bytes()[..64].iter().all(u8::is_ascii_hexdigit)
                && e.file_type().is_ok_and(|t| t.is_file())
        })
        .collect::<Vec<_>>();
    if entries.len() > 128 {
        entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        for e in entries.iter().take(entries.len() - 128) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(())
}
async fn get(c: Option<&Core>, url: &str, max: usize, refresh: bool) -> Result<Vec<u8>> {
    if !refresh {
        if let Some(c) = c {
            if let Some(bytes) = cached(c, url, max) {
                return Ok(bytes);
            }
        }
    }
    let response = maps::client()?
        .get(url)
        .timeout(Duration::from_secs(12))
        .send()
        .await
        .context("Le fond OpenStreetMap vectoriel ne répond pas")?;
    let bytes = maps::limited(response, max).await?;
    if let Some(c) = c {
        store(c, url, &bytes)?;
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<Tile> {
    let mut plain = Vec::new();
    let data = if bytes.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_DECODED as u64 + 1)
            .read_to_end(&mut plain)?;
        plain.as_slice()
    } else {
        bytes
    };
    if data.len() > MAX_DECODED {
        bail!("Tuile vectorielle trop volumineuse");
    }
    let tile = Tile::decode(data).context("Tuile OpenStreetMap non reconnue")?;
    if tile.layers.len() > 100 {
        bail!("Trop de couches dans la tuile");
    }
    Ok(tile)
}
fn attribute<'a>(layer: &'a Layer, feature: &TileFeature, key: &str) -> Option<&'a str> {
    feature.tags.chunks_exact(2).find_map(|pair| {
        if layer.keys.get(pair[0] as usize)?.as_str() != key {
            return None;
        }
        layer.values.get(pair[1] as usize)?.text.as_deref()
    })
}
fn segments(commands: &[u32]) -> Result<Vec<Vec<[i64; 2]>>> {
    let mut parts = vec![];
    let mut current = vec![];
    let (mut x, mut y, mut index) = (0i64, 0i64, 0usize);
    if commands.len() > 200_000 {
        bail!("Géométrie de tuile trop volumineuse");
    }
    while index < commands.len() {
        let command = commands[index];
        index += 1;
        let id = command & 7;
        let count = (command >> 3) as usize;
        if count == 0 {
            bail!("Commande géométrique vide");
        }
        match id {
            1 | 2 => {
                if count > (commands.len() - index) / 2 {
                    bail!("Géométrie tronquée");
                }
                for _ in 0..count {
                    let zig = |v: u32| ((v >> 1) as i64) ^ -((v & 1) as i64);
                    x = x
                        .checked_add(zig(commands[index]))
                        .context("Coordonnée excessive")?;
                    y = y
                        .checked_add(zig(commands[index + 1]))
                        .context("Coordonnée excessive")?;
                    index += 2;
                    if x.abs() > 16_777_216 || y.abs() > 16_777_216 {
                        bail!("Coordonnée de tuile hors limites");
                    }
                    if id == 1 && !current.is_empty() {
                        parts.push(std::mem::take(&mut current));
                    }
                    if id == 2 && current.is_empty() {
                        bail!("Segment sans origine");
                    }
                    current.push([x, y]);
                }
            }
            7 => {
                if count != 1 || current.len() < 3 {
                    bail!("Contour de bâtiment invalide");
                }
                if current.last() != current.first() {
                    current.push(current[0]);
                }
            }
            _ => bail!("Commande géométrique inconnue"),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}
fn lon_lat(t: TileId, extent: u32, p: [i64; 2]) -> [f64; 2] {
    let n = (1u32 << t.z) as f64;
    let world_x = (t.x as f64 + p[0] as f64 / extent as f64) / n;
    let world_y = (t.y as f64 + p[1] as f64 / extent as f64) / n;
    [
        (world_x * 360. - 180.).clamp(-180., 180.),
        ((std::f64::consts::PI * (1. - 2. * world_y))
            .sinh()
            .atan()
            .to_degrees())
        .clamp(-85., 85.),
    ]
}
fn intersects(points: &[[f64; 2]], b: [f64; 4]) -> bool {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for p in points {
        for axis in 0..2 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    max[0] >= b[0] && min[0] <= b[2] && max[1] >= b[1] && min[1] <= b[3]
}
fn append(tile: Tile, id: TileId, plan: &mut Plan, points_used: &mut usize) -> Result<()> {
    // Les voies nommées passent en premier pour conserver les libellés en cas de limite.
    for layer_name in ["transportation_name", "transportation", "building"] {
        for layer in tile.layers.iter().filter(|l| l.name == layer_name) {
            if !(1..=2).contains(&layer.version) {
                bail!("Version de tuile non prise en charge");
            }
            let extent = layer.extent.unwrap_or(4096);
            if extent == 0 || extent > 65536 {
                bail!("Échelle de tuile invalide");
            }
            for f in &layer.features {
                let building = layer_name == "building";
                if f.kind != Some(if building { 3 } else { 2 }) {
                    continue;
                }
                if f.tags.len() % 2 != 0 {
                    bail!("Attributs de tuile invalides");
                }
                if !building
                    && attribute(layer, f, "class")
                        .is_some_and(|v| ["rail", "transit", "aerialway", "ferry"].contains(&v))
                {
                    continue;
                }
                let name = ["name:fr", "name", "name:latin"]
                    .iter()
                    .find_map(|k| attribute(layer, f, k))
                    .unwrap_or("");
                // Limite en octets compatible avec la validation de toutes les entrées du registre.
                let name = name
                    .chars()
                    .scan(0, |size, c| {
                        *size += c.len_utf8();
                        (*size <= 250).then_some(c)
                    })
                    .collect::<String>();
                for part in segments(&f.geometry)? {
                    if part.len() < if building { 4 } else { 2 } {
                        continue;
                    }
                    if building {
                        let area: i128 = part
                            .windows(2)
                            .map(|p| {
                                p[0][0] as i128 * p[1][1] as i128
                                    - p[1][0] as i128 * p[0][1] as i128
                            })
                            .sum();
                        if area <= 0 {
                            continue;
                        }
                    }
                    let points = part
                        .into_iter()
                        .map(|p| lon_lat(id, extent, p))
                        .collect::<Vec<_>>();
                    if !intersects(&points, plan.bounds) {
                        continue;
                    }
                    if plan.features.len() >= 2000 || *points_used + points.len() > 50000 {
                        plan.truncated = true;
                        continue;
                    }
                    *points_used += points.len();
                    plan.features.push(Feature {
                        kind: if building { "building" } else { "road" }.into(),
                        name: name.clone(),
                        points,
                    });
                }
            }
        }
    }
    Ok(())
}
pub async fn fetch(c: Option<&Core>, endpoint: &str, bounds: [f64; 4]) -> Result<Plan> {
    maps::endpoint(endpoint)?;
    // Le catalogue est petit et désigne la version hebdomadaire des tuiles.
    let bytes = get(None, endpoint, 1024 * 1024, true).await?;
    let meta: Value =
        serde_json::from_slice(&bytes).context("Catalogue de tuiles OpenStreetMap invalide")?;
    let template = meta["tiles"]
        .as_array()
        .and_then(|v| v.first())
        .and_then(Value::as_str)
        .context("Adresse des tuiles absente")?;
    let max_zoom = meta["maxzoom"].as_u64().unwrap_or(14).min(14) as u32;
    let ids = tile_ids(bounds, max_zoom)?;
    let mut plan = Plan {
        source: "osm-vector".into(),
        bounds,
        features: vec![],
        obtained_at: db::now(),
        truncated: false,
    };
    let mut count = 0;
    for id in ids {
        let url = tile_url(template, endpoint, id)?;
        let bytes = get(c, &url, MAX_TILE, false).await?;
        append(decode(&bytes)?, id, &mut plan, &mut count)?;
    }
    if plan.features.is_empty() {
        bail!("Aucune route ni bâtiment OpenStreetMap dans cette zone");
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_cover_public_coordinates_without_bulk_download() {
        for (lat, lon) in [
            (48.8566, 2.3522),
            (43.4, -0.4),
            (85., 179.99),
            (-85., -179.99),
        ] {
            let ids = tile_ids(maps::bounds(lat, lon), 14).unwrap();
            assert!(!ids.is_empty() && ids.len() <= 4);
            for id in ids {
                assert!(id.x < 1 << id.z && id.y < 1 << id.z);
            }
        }
    }
    #[test]
    fn tile_addresses_cannot_redirect_to_another_host() {
        let id = TileId {
            z: 14,
            x: 8299,
            y: 5636,
        };
        assert!(tile_url(
            "https://tiles.openfreemap.org/planet/a/{z}/{x}/{y}.pbf",
            ENDPOINT,
            id
        )
        .is_ok());
        assert!(tile_url("https://other.example/{z}/{x}/{y}.pbf", ENDPOINT, id).is_err());
        assert!(tile_url("https://tiles.openfreemap.org/{z}/{x}.pbf", ENDPOINT, id).is_err());
    }
    #[test]
    fn malformed_geometries_are_rejected() {
        assert!(segments(&[9, 2]).is_err());
        assert!(segments(&[10, 2, 2]).is_err());
        assert!(segments(&[0]).is_err());
        assert!(segments(&[15]).is_err());
        let p = segments(&[9, 20, 20, 18, 20, 0, 0, 20, 15]).unwrap();
        assert_eq!(p[0], vec![[10, 10], [20, 10], [20, 20], [10, 10]]);
    }
    #[test]
    fn decoded_roads_use_real_world_coordinates() {
        let tile = Tile {
            layers: vec![Layer {
                version: 2,
                name: "transportation_name".into(),
                extent: Some(4096),
                keys: vec!["name".into()],
                values: vec![TileValue {
                    text: Some("Rue fictive".into()),
                }],
                features: vec![TileFeature {
                    kind: Some(2),
                    tags: vec![0, 0],
                    geometry: vec![9, 4096, 4096, 10, 200, 0],
                }],
            }],
        };
        let bytes = tile.encode_to_vec();
        let mut plan = Plan {
            source: "osm-vector".into(),
            bounds: [-1., -1., 1., 1.],
            features: vec![],
            obtained_at: db::now(),
            truncated: false,
        };
        append(
            decode(&bytes).unwrap(),
            TileId { z: 0, x: 0, y: 0 },
            &mut plan,
            &mut 0,
        )
        .unwrap();
        assert_eq!(plan.features.len(), 1);
        assert_eq!(plan.features[0].name, "Rue fictive");
        assert_eq!(plan.features[0].points[0], [0., 0.]);
    }

    #[tokio::test]
    async fn tile_cache_is_encrypted_and_can_be_read_without_network() {
        let root = std::env::temp_dir().join(format!("libre-erp-map-cache-{}", db::id()));
        let c = crate::initialise(root.clone()).await.unwrap();
        let address = "https://tiles.openfreemap.org/example/14/0/0.pbf";
        let bytes = b"Geometrie publique fictive de test";
        store(&c, address, bytes).unwrap();
        assert_eq!(cached(&c, address, MAX_TILE).unwrap(), bytes);
        let raw = std::fs::read(cache_path(&c, address)).unwrap();
        assert!(!raw.windows(bytes.len()).any(|p| p == bytes));
        assert!(cached(&c, address, 2).is_none());
        c.pool.close().await;
        drop(c);
        assert!(root.starts_with(std::env::temp_dir()));
        assert!(root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("libre-erp-map-cache-"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn damaged_or_oversized_tiles_are_rejected() {
        use std::io::Write;
        assert!(decode(&[255, 255]).is_err());
        let mut zipped = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        zipped.write_all(&vec![0; MAX_DECODED + 1]).unwrap();
        assert!(decode(&zipped.finish().unwrap()).is_err());
    }
    #[tokio::test]
    #[ignore = "Recette publique OpenFreeMap demandée"]
    async fn live_vector_public_paris() {
        let p = fetch(None, ENDPOINT, maps::bounds(48.8566, 2.3522))
            .await
            .unwrap();
        assert_eq!(p.source, "osm-vector");
        assert!(p.features.iter().any(|f| f.kind == "building"));
        assert!(p.features.iter().any(|f| f.kind == "road"));
        maps::validate(&Some(maps::Location {
            lat: 48.8566,
            lon: 2.3522,
            source_address: "Hôtel de Ville de Paris (exemple public)".into(),
            plan: Some(p.clone()),
        }))
        .unwrap();
        if let Some(path) = std::env::var_os("ERP_MAP_TEST_OUTPUT") {
            std::fs::write(path, serde_json::to_vec(&p).unwrap()).unwrap();
        }
        println!(
            "Fond OSM vectoriel public : {} objets, {} libellés",
            p.features.len(),
            p.features.iter().filter(|f| !f.name.is_empty()).count()
        );
    }
}

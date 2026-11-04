//! Géocodage volontaire et plan vectoriel local OSM ; aucun téléchargement de tuiles.
use crate::{auth, db, Core};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Feature {
    pub kind: String,
    pub name: String,
    pub points: Vec<[f64; 2]>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Plan {
    #[serde(default = "osm_source")]
    pub source: String,
    pub bounds: [f64; 4],
    pub features: Vec<Feature>,
    pub obtained_at: String,
    pub truncated: bool,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub source_address: String,
    pub plan: Option<Plan>,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Candidate {
    pub lat: f64,
    pub lon: f64,
    pub label: String,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Services {
    #[serde(default = "ign_endpoint")]
    pub ign: String,
    #[serde(default = "yes")]
    pub ign_fallback: bool,
    pub geocoder: String,
    pub geometry: String,
}
fn osm_source() -> String {
    "osm".into()
}
fn ign_endpoint() -> String {
    "https://data.geopf.fr/wfs/ows".into()
}
fn yes() -> bool {
    true
}
impl Default for Services {
    fn default() -> Self {
        Self {
            ign: ign_endpoint(),
            ign_fallback: true,
            geocoder: "https://nominatim.openstreetmap.org/search".into(),
            geometry: "https://overpass-api.de/api/interpreter".into(),
        }
    }
}
pub fn point(lat: f64, lon: f64) -> Result<()> {
    if !lat.is_finite()
        || !lon.is_finite()
        || !(-85.0..=85.0).contains(&lat)
        || !(-180.0..=180.0).contains(&lon)
    {
        bail!("Latitude ou longitude invalide (latitude entre -85 et 85, longitude entre -180 et 180).")
    };
    Ok(())
}
pub fn bounds(lat: f64, lon: f64) -> [f64; 4] {
    let dx = 0.003 / (lat.to_radians().cos().max(0.1));
    [
        (lon - dx).max(-180.),
        (lat - 0.0017).max(-85.),
        (lon + dx).min(180.),
        (lat + 0.0017).min(85.),
    ]
}
pub fn validate(location: &Option<Location>) -> Result<()> {
    if let Some(l) = location {
        point(l.lat, l.lon)?;
        if l.source_address.len() > 1000 {
            bail!("Adresse de localisation trop longue")
        };
        if let Some(p) = &l.plan {
            if !["osm", "ign"].contains(&p.source.as_str()) {
                bail!("Provenance du plan inconnue.")
            }
            let [w, s, e, n] = p.bounds;
            if ![w, s, e, n].iter().all(|v| v.is_finite())
                || w >= e
                || s >= n
                || w < -180.
                || e > 180.
                || s < -85.
                || n > 85.
                || e - w > 0.061
                || n - s > 0.004
                || l.lon < w
                || l.lon > e
                || l.lat < s
                || l.lat > n
            {
                bail!("Emprise du plan invalide.")
            }
            if p.features.len() > 2000
                || p.features.iter().map(|f| f.points.len()).sum::<usize>() > 50000
                || p.obtained_at.len() > 50
            {
                bail!("Plan trop volumineux.")
            }
            for f in &p.features {
                if !["road", "building"].contains(&f.kind.as_str())
                    || f.name.len() > 250
                    || f.points.len() < 2
                {
                    bail!("Objet du plan invalide.")
                }
                for [x, y] in &f.points {
                    point(*y, *x)?;
                }
            }
        }
    }
    Ok(())
}
fn endpoint(value: &str) -> Result<()> {
    let u = url::Url::parse(value)?;
    if u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || u.query().is_some()
    {
        bail!("Indiquez un service HTTPS sans identifiant, paramètres ni fragment.")
    }
    Ok(())
}
pub async fn settings(c: &Core) -> Result<Services> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE key='maps.services'")
            .fetch_optional(&c.pool)
            .await?;
    let mut services: Services = value
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    if legacy_geometry(&services.geometry) {
        services.geometry = Services::default().geometry;
    }
    Ok(services)
}
pub async fn save_settings(c: &Core, token: &str, input: Services) -> Result<()> {
    crate::shared_workspace::require_write(c)?;
    let actor = auth::require(c, token, Some("users.manage")).await?;
    endpoint(&input.geocoder)?;
    endpoint(&input.geometry)?;
    endpoint(&input.ign)?;
    let mut tx = c.pool.begin().await?;
    sqlx::query("DELETE FROM settings WHERE key='maps.overpass_pause'").execute(&mut *tx).await?;
    sqlx::query("INSERT INTO settings(key,value_json,modified_at,modified_by) VALUES('maps.services',?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at,modified_by=excluded.modified_by").bind(serde_json::to_string(&input)?).bind(db::now()).bind(&actor).execute(&mut *tx).await?;
    crate::audit::append(
        &mut tx,
        &c.key,
        Some(&actor),
        "maps.services",
        json!({"geocoder":input.geocoder,"geometry":input.geometry,"ign":input.ign,"ign_fallback":input.ign_fallback}),
    )
    .await?;
    tx.commit().await?;
    crate::audit::checkpoint(&c.pool, &c.root).await
}
pub(crate) fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!(
            "LibreERP/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/LaurentN1305/libre-erp)"
        ))
        .timeout(std::time::Duration::from_secs(22))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
pub(crate) async fn limited(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    if !response.status().is_success() {
        bail!(
            "Service cartographique indisponible (HTTP {}). Vos données locales sont conservées.",
            response.status()
        )
    }
    if response.content_length().is_some_and(|n| n > max as u64) {
        bail!("Réponse cartographique trop volumineuse.")
    }
    let mut out = vec![];
    while let Some(chunk) = response.chunk().await? {
        if out.len() + chunk.len() > max {
            bail!("Réponse cartographique trop volumineuse.")
        }
        out.extend_from_slice(&chunk)
    }
    Ok(out)
}
async fn permitted(c: &Core, token: &str) -> Result<String> {
    auth::require(c, token, Some("buildings.write")).await
}
async fn throttle(c: &Core, key: &str, millis: i64) -> Result<()> {
    let now = chrono::Utc::now().timestamp_millis();
    let last: Option<String> = sqlx::query_scalar("SELECT value_json FROM settings WHERE key=?")
        .bind(key)
        .fetch_optional(&c.pool)
        .await?;
    if last
        .and_then(|s| s.parse::<i64>().ok())
        .is_some_and(|v| now - v < millis)
    {
        bail!("Attendez quelques secondes avant une nouvelle recherche.")
    }
    sqlx::query("INSERT INTO settings(key,value_json,modified_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,modified_at=excluded.modified_at").bind(key).bind(now.to_string()).bind(db::now()).execute(&c.pool).await?;
    Ok(())
}
pub fn candidates(value: Value) -> Result<Vec<Candidate>> {
    let rows = value.as_array().context("Réponse de géocodage invalide")?;
    let mut out = vec![];
    for v in rows.iter().take(5) {
        let lat = v["lat"].as_str().context("Latitude absente")?.parse()?;
        let lon = v["lon"].as_str().context("Longitude absente")?.parse()?;
        point(lat, lon)?;
        let label = v["display_name"].as_str().context("Adresse absente")?;
        out.push(Candidate {
            lat,
            lon,
            label: label.chars().take(500).collect(),
        });
    }
    Ok(out)
}
pub async fn geocode(c: &Core, token: &str, address: &str) -> Result<Vec<Candidate>> {
    let actor = permitted(c, token).await?;
    let query = address.split_whitespace().collect::<Vec<_>>().join(" ");
    if query.len() < 5 || query.len() > 1000 {
        bail!("Renseignez une adresse avec sa commune.")
    }
    let org: String = sqlx::query_scalar("SELECT organisation_id FROM users WHERE id=?")
        .bind(actor)
        .fetch_one(&c.pool)
        .await?;
    let service = settings(c).await?;
    let cache_key = format!("{}|{}", service.geocoder, query.to_lowercase());
    if let Some(r)=sqlx::query("SELECT results_json FROM map_geocache WHERE organisation_id=? AND query=? AND fetched_at>?").bind(&org).bind(&cache_key).bind((chrono::Utc::now()-chrono::Duration::days(30)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)).fetch_optional(&c.pool).await?{return Ok(serde_json::from_str(&r.get::<String,_>(0))?)}
    throttle(c, "maps.last_geocode", 1100).await?;
    endpoint(&service.geocoder)?;
    let data = limited(
        client()?
            .get(&service.geocoder)
            .query(&[
                ("q", query.as_str()),
                ("format", "jsonv2"),
                ("limit", "5"),
                ("accept-language", "fr"),
            ])
            .send()
            .await?,
        256 * 1024,
    )
    .await?;
    let result = candidates(serde_json::from_slice(&data)?)?;
    sqlx::query("INSERT INTO map_geocache(organisation_id,query,results_json,fetched_at) VALUES(?,?,?,?) ON CONFLICT(organisation_id,query) DO UPDATE SET results_json=excluded.results_json,fetched_at=excluded.fetched_at").bind(org).bind(cache_key).bind(serde_json::to_string(&result)?).bind(db::now()).execute(&c.pool).await?;
    Ok(result)
}
pub fn parse_plan(value: Value, extent: [f64; 4]) -> Result<Plan> {
    let elements = value["elements"]
        .as_array()
        .context("Réponse de plan invalide")?;
    let mut features = vec![];
    let mut count = 0;
    let mut truncated = false;
    for e in elements {
        let kind = if e["tags"]["building"].is_string() {
            "building"
        } else if e["tags"]["highway"].is_string() {
            "road"
        } else {
            continue;
        };
        let Some(g) = e["geometry"].as_array() else {
            continue;
        };
        if g.len() < 2 {
            continue;
        }
        if features.len() >= 2000 || count + g.len() > 50000 {
            truncated = true;
            break;
        }
        let mut points = vec![];
        for p in g {
            let lat = p["lat"].as_f64().context("Latitude du plan invalide")?;
            let lon = p["lon"].as_f64().context("Longitude du plan invalide")?;
            point(lat, lon)?;
            points.push([lon, lat]);
        }
        count += points.len();
        features.push(Feature {
            kind: kind.into(),
            name: e["tags"]["name"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(250)
                .collect(),
            points,
        });
    }
    Ok(Plan {
        source: "osm".into(),
        bounds: extent,
        features,
        obtained_at: db::now(),
        truncated,
    })
}
pub async fn context(c: &Core, token: &str, lat: f64, lon: f64) -> Result<Plan> {
    permitted(c, token).await?;
    point(lat, lon)?;
    throttle(c, "maps.last_context", 2500).await?;
    let b = bounds(lat, lon);
    let services = settings(c).await?;
    endpoint(&services.geometry)?;
    endpoint(&services.ign)?;
    // A user-requested refresh always attempts OSM: no hidden five-minute bypass.
    let primary = geometry_plan(&services.geometry, b).await;
    let plan = match primary {
        Ok(plan) => plan,
        Err(error) => {
            if !services.ign_fallback { return Err(error); }
            crate::maps_ign::fetch(&services.ign,b).await.map_err(|_|anyhow::anyhow!("OpenStreetMap : {}. Le secours IGN est également indisponible. Les coordonnées et le plan précédent sont conservés.", error))?
        }
    };
    validate(&Some(Location {
        lat,
        lon,
        source_address: String::new(),
        plan: Some(plan.clone()),
    }))?;
    Ok(plan)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_and_osm_plan_are_bounded() {
        assert!(point(f64::NAN, 2.).is_err());
        assert!(point(91., 2.).is_err());
        let b = bounds(48.85, 2.35);
        let p=parse_plan(json!({"elements":[{"tags":{"highway":"residential","name":"Rue test"},"geometry":[{"lat":48.85,"lon":2.35},{"lat":48.851,"lon":2.351}]},{"tags":{"building":"yes"},"geometry":[{"lat":48.85,"lon":2.35},{"lat":48.8501,"lon":2.35},{"lat":48.85,"lon":2.35}]}]}),b).unwrap();
        assert_eq!(p.features.len(), 2);
        assert!(validate(&Some(Location {
            lat: 48.85,
            lon: 2.35,
            source_address: "Adresse test".into(),
            plan: Some(p)
        }))
        .is_ok());
        assert!(endpoint("http://example.org").is_err());
        assert!(endpoint("https://user:secret@example.org").is_err());
        assert_eq!(
            candidates(json!([{"lat":"48.85","lon":"2.35","display_name":"Paris"}]))
                .unwrap()
                .len(),
            1
        );
    }
}
#[cfg(all(test, windows))]
mod integration {
    use super::*;
    async fn setup() -> (Core, String) {
        let root = std::env::temp_dir().join(format!("erp-cartographie-{}", db::id()));
        let c = crate::initialise(root).await.unwrap();
        let token = crate::auth::bootstrap(
            &c,
            "Mairie test",
            "Admin",
            "admin",
            "Phrase administrateur test",
        )
        .await
        .unwrap()
        .token;
        (c, token)
    }
    #[tokio::test]
    async fn location_and_entry_shortcuts_respect_scope() {
        let (c, t) = setup().await;
        let input = crate::dashboard::BuildingInput {
            id: None,
            name: "Bâtiment localisé".into(),
            address: "Place centrale".into(),
            category: 5,
            capacity: 100,
            types: vec!["L".into(), "X".into()],
            manager: None,
            opened_on: None,
            location: Some(Location {
                lat: 48.8566,
                lon: 2.3522,
                source_address: "Place centrale".into(),
                plan: None,
            }),
        };
        let id = crate::dashboard::save(&c, &t, input).await.unwrap();
        let snap = crate::dashboard::snapshot(&c, &t).await.unwrap();
        assert_eq!(snap["buildings"][0]["location"]["lat"], 48.8566);
        assert_eq!(snap["buildings"][0]["can_add_work"], true);
        assert_eq!(snap["buildings"][0]["register_read"], true);
        crate::auth::create_user(
            &c,
            &t,
            "Commission",
            "commission",
            "Phrase commission test 123",
            "commission",
        )
        .await
        .unwrap();
        let visitor = crate::auth::login(&c, "commission", "Phrase commission test 123", None)
            .await
            .unwrap();
        let snap = crate::dashboard::snapshot(&c, &visitor.token)
            .await
            .unwrap();
        assert_eq!(snap["buildings"][0]["can_add_work"], false);
        assert_eq!(snap["buildings"][0]["register_read"], true);
        assert!(
            geocode(&c, &visitor.token, "Adresse qui ne doit pas être envoyée")
                .await
                .is_err()
        );
        crate::dashboard::archive(&c, &t, &id, true).await.unwrap();
        let snap = crate::dashboard::snapshot(&c, &t).await.unwrap();
        assert_eq!(snap["buildings"][0]["can_add_work"], false);
        let r = c.root.clone();
        c.pool.close().await;
        drop(c);
        std::fs::remove_dir_all(r).unwrap();
    }
    #[tokio::test]
    #[ignore = "Appels publics réels, uniquement sur demande explicite de recette"]
    async fn online_public_paris_example() {
        let (c, t) = setup().await;
        let result: Result<()> = async {
            let mut config=Services::default();config.geometry="https://127.0.0.1:9/overpass-unavailable".into();save_settings(&c,&t,config).await?;
            let r = geocode(&c, &t, "Hotel de Ville Paris France").await?;
            if r.is_empty() {
                bail!("Aucun résultat public")
            };
            let p = context(&c, &t, r[0].lat, r[0].lon).await?;
            assert_eq!(p.source,"ign");
            if let Some(path)=std::env::var_os("ERP_MAP_TEST_OUTPUT"){std::fs::write(path,serde_json::to_vec(&serde_json::json!({"lat":r[0].lat,"lon":r[0].lon,"source_address":"Hôtel de Ville, Paris (exemple public)","plan":p}))?)?;}
            if p.features.is_empty() {
                bail!("Plan public vide")
            };
            println!(
                "Exemple public, secours IGN : {} résultats, {} objets",
                r.len(),
                p.features.len()
            );
            Ok(())
        }
        .await;
        let root = c.root.clone();
        c.pool.close().await;
        drop(c);
        std::fs::remove_dir_all(root).unwrap();
        result.unwrap();
    }
}

pub(crate) fn parse_direct(mut value: Value, b: [f64; 4]) -> Result<Plan> {
    let elements = value["elements"]
        .as_array()
        .context("Réponse OpenStreetMap invalide")?;
    let nodes: std::collections::HashMap<i64, Value> = elements
        .iter()
        .filter(|e| e["type"] == "node")
        .filter_map(|n| {
            n["id"]
                .as_i64()
                .map(|id| (id, json!({"lat":n["lat"],"lon":n["lon"]})))
        })
        .collect();
    let mut ways = vec![];
    for element in elements.iter().filter(|e| {
        e["type"] == "way"
            && (e["tags"]["building"].is_string() || e["tags"]["highway"].is_string())
    }) {
        let mut way = element.clone();
        let ids = way["nodes"]
            .as_array()
            .context("Géométrie OpenStreetMap invalide")?;
        let points: Vec<_> = ids
            .iter()
            .filter_map(|id| nodes.get(&id.as_i64()?).cloned())
            .collect();
        if points.len() != ids.len() {
            continue;
        };
        way["geometry"] = json!(points);
        ways.push(way)
    }
    value["elements"] = json!(ways);
    parse_plan(value, b)
}
pub(crate) async fn direct_plan(url: &str, b: [f64; 4]) -> Result<Plan> {
    let response = client()?
        .get(url)
        .header("Accept", "application/json")
        .query(&[("bbox", format!("{},{},{},{}", b[0], b[1], b[2], b[3]))])
        .timeout(std::time::Duration::from_secs(18))
        .send()
        .await
        .context("Connexion OpenStreetMap impossible")?;
    let bytes = limited(response, 8 * 1024 * 1024).await?;
    let p = parse_direct(serde_json::from_slice(&bytes)?, b)?;
    if p.features.is_empty() {
        bail!("Aucun objet OpenStreetMap dans cette zone")
    };
    Ok(p)
}
#[cfg(test)]
mod direct_tests {
    use super::*;
    #[test]
    fn reconstructs_ways_without_storing_contributor_metadata() {
        let p=parse_direct(json!({"elements":[{"type":"node","id":1,"lat":48.856,"lon":2.352,"user":"private"},{"type":"node","id":2,"lat":48.857,"lon":2.353},{"type":"way","id":3,"nodes":[1,2],"tags":{"highway":"service"}}]}),bounds(48.856,2.352)).unwrap();
        assert_eq!(p.source, "osm");
        assert_eq!(p.features.len(), 1);
        assert!(!serde_json::to_string(&p).unwrap().contains("private"));
    }
    #[tokio::test]
    #[ignore = "Recette réseau réelle volontaire"]
    async fn live_direct_osm() {
        let b = bounds(48.8566, 2.3522);
        let p = direct_plan("https://api.openstreetmap.org/api/0.6/map.json", b)
            .await
            .unwrap();
        assert!(!p.features.is_empty());
        assert_eq!(p.source, "osm");
        println!("OSM direct natif : {} objets", p.features.len());
    }
}

fn legacy_geometry(url: &str) -> bool {
    matches!(url, "https://overpass.private.coffee/api/interpreter" | "https://api.openstreetmap.org/api/0.6/map.json")
}
async fn geometry_plan(url: &str, b: [f64;4]) -> Result<Plan> {
    endpoint(url)?;
    if url.ends_with("/api/0.6/map.json") { return direct_plan(url,b).await; }
    let query=format!("[out:json][timeout:8][maxsize:16777216];(way[highway]({},{},{},{});way[building]({},{},{},{}););out geom;", b[1],b[0],b[3],b[2],b[1],b[0],b[3],b[2]);
    // GET also works behind networks which disallow form POSTs to map services.
    let response=client()?.get(url).query(&[("data",query)]).timeout(std::time::Duration::from_secs(14)).send().await
        .map_err(|e|anyhow::anyhow!("{}", if e.is_timeout(){"Délai de réponse OpenStreetMap dépassé (14 s)"}else if e.is_connect(){"Connexion au service OpenStreetMap impossible"}else{"Réponse OpenStreetMap indisponible"}))?;
    let bytes=limited(response,3*1024*1024).await?;
    let plan=parse_plan(serde_json::from_slice(&bytes).context("Réponse JSON OpenStreetMap invalide")?,b)?;
    if plan.features.is_empty(){bail!("Aucun objet OpenStreetMap dans cette zone")}
    Ok(plan)
}
#[derive(Serialize)]
pub struct Diagnostic { service:String, ok:bool, detail:String }
pub async fn diagnostics(c:&Core,token:&str)->Result<Vec<Diagnostic>> {
    auth::require(c,token,Some("users.manage")).await?;
    throttle(c,"maps.last_diagnostic",30_000).await?;
    let service=settings(c).await?;
    let b=bounds(48.8566,2.3522);
    let mut result=vec![];
    let query=client()?.get(&service.geocoder).query(&[("q","Hotel de Ville Paris France"),("format","jsonv2"),("limit","1")]).timeout(std::time::Duration::from_secs(10)).send().await;
    let geo:Result<usize>=async{let bytes=limited(query.context("Connexion au géocodage impossible ou délai dépassé")?,256*1024).await?;Ok(candidates(serde_json::from_slice(&bytes)?)?.len())}.await;
    result.push(match geo {Ok(n)=>Diagnostic{service:"Recherche d’adresse OpenStreetMap".into(),ok:n>0,detail:format!("{} résultat(s) pour l’exemple public Hôtel de Ville de Paris",n)},Err(e)=>Diagnostic{service:"Recherche d’adresse OpenStreetMap".into(),ok:false,detail:e.to_string()}});
    for (name,test) in [("Plan OpenStreetMap",geometry_plan(&service.geometry,b).await),("Plan IGN",crate::maps_ign::fetch(&service.ign,b).await)]{
        result.push(match test {Ok(p)=>Diagnostic{service:name.into(),ok:true,detail:format!("{} objets reçus et interprétés",p.features.len())},Err(e)=>Diagnostic{service:name.into(),ok:false,detail:e.to_string()}});
    }
    Ok(result)
}
#[cfg(test)]
mod service_tests {
 use super::*;
 #[test] fn replaces_only_obsolete_public_endpoints(){assert!(legacy_geometry("https://api.openstreetmap.org/api/0.6/map.json"));assert!(legacy_geometry("https://overpass.private.coffee/api/interpreter"));assert!(!legacy_geometry("https://mon-service.fr/api/interpreter"));}
 #[tokio::test] #[ignore="Recette réseau OSM volontaire"] async fn live_readonly_osm(){let p=geometry_plan(&Services::default().geometry,bounds(48.8566,2.3522)).await.unwrap();assert_eq!(p.source,"osm");assert!(!p.features.is_empty());}
}

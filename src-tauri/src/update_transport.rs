//! Transport HTTPS borné, avec redirections GitHub Releases explicitement autorisées.
use anyhow::{bail, Context, Result};
use std::time::Duration;
use url::Url;
fn secure(u: &Url) -> bool {
    u.scheme() == "https"
        && u.port_or_known_default() == Some(443)
        && u.username().is_empty()
        && u.password().is_none()
        && u.fragment().is_none()
}
#[derive(Clone)]
pub struct Origin {
    base: Url,
    github: bool,
}
impl Origin {
    pub fn parse(s: &str) -> Result<Self> {
        let base = Url::parse(s)?;
        if !secure(&base) || base.query().is_some() {
            bail!("Serveur de publication HTTPS invalide")
        };
        let github = base.host_str() == Some("github.com");
        if github {
            let p: Vec<_> = base.path().trim_matches('/').split('/').collect();
            if p.len() != 4
                || p[0] != "LaurentN1305"
                || p[1] != "libre-erp"
                || p[2] != "releases"
                || p[3] != "download"
            {
                bail!("Dépôt GitHub de publication non autorisé")
            }
        };
        Ok(Self { base, github })
    }
    pub fn initial(&self, u: &Url) -> bool {
        secure(u)
            && u.host_str() == self.base.host_str()
            && u.query().is_none()
            && (!self.github
                || u.path()
                    .starts_with(&format!("{}/", self.base.path().trim_end_matches('/'))))
    }
    fn redirect(&self, u: &Url) -> bool {
        secure(u)
            && (u.host_str() == self.base.host_str()
                || self.github
                    && matches!(
                        u.host_str(),
                        Some(
                            "release-assets.githubusercontent.com"
                                | "objects.githubusercontent.com"
                        )
                    ))
    }
    pub fn client(&self, timeout: Duration) -> Result<reqwest::Client> {
        let allowed = self.clone();
        Ok(reqwest::Client::builder()
            .user_agent("LibreERP-Updater")
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() > 3 {
                    attempt.error("Trop de redirections")
                } else if allowed.redirect(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("Redirection de téléchargement non autorisée")
                }
            }))
            .build()?)
    }
    pub fn release(&self, url: &str) -> Result<Url> {
        let u = Url::parse(url)?;
        if !self.initial(&u) {
            bail!("Hôte ou dépôt de téléchargement non autorisé")
        };
        Ok(u)
    }
}
pub fn configured() -> Result<Origin> {
    Origin::parse(option_env!("ERP_UPDATE_BASE_URL").context("Serveur absent")?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn github_download_routes_are_bounded() {
        let o =
            Origin::parse("https://github.com/LaurentN1305/libre-erp/releases/download").unwrap();
        assert!(o
            .release(
                "https://github.com/LaurentN1305/libre-erp/releases/download/v0.11.0/setup.exe"
            )
            .is_ok());
        for u in [
            "https://github.com/autre/projet/releases/download/v1/setup.exe",
            "http://github.com/LaurentN1305/libre-erp/releases/download/v1/setup.exe",
            "https://evil.invalid/setup.exe",
            "https://user@github.com/LaurentN1305/libre-erp/releases/download/v1/setup.exe",
        ] {
            assert!(o.release(u).is_err(), "{u}")
        }
        assert!(o.redirect(&Url::parse("https://release-assets.githubusercontent.com/github-production-release-asset/test?token=ephemere").unwrap()));
        assert!(!o.redirect(
            &Url::parse("https://release-assets.githubusercontent.com.evil.invalid/a").unwrap()
        ));
        assert!(!o.redirect(&Url::parse("http://release-assets.githubusercontent.com/a").unwrap()));
        assert!(Origin::parse("https://github.com/autre/projet/releases/download").is_err());
    }
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FrameworkKind {
    NextJs,
    Nuxt,
    SvelteKit,
    Remix,
    Astro,
    Vite,
    Gatsby,
    Angular,
    Express,
    StaticHtml,
    GenericNode,
    Unknown,
}

impl fmt::Display for FrameworkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(framework_display_name(self))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeployMode {
    Static,
    Dynamic(DynamicConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicConfig {
    pub framework: FrameworkKind,
    pub start_command: Vec<String>,
    pub health_path: String,
    pub output_dir: String,
    pub env_overrides: HashMap<String, String>,
}

pub fn framework_display_name(kind: &FrameworkKind) -> &'static str {
    match kind {
        FrameworkKind::NextJs => "Next.js",
        FrameworkKind::Nuxt => "Nuxt",
        FrameworkKind::SvelteKit => "SvelteKit",
        FrameworkKind::Remix => "Remix",
        FrameworkKind::Astro => "Astro",
        FrameworkKind::Vite => "Vite",
        FrameworkKind::Gatsby => "Gatsby",
        FrameworkKind::Angular => "Angular",
        FrameworkKind::Express => "Express",
        FrameworkKind::StaticHtml => "Static HTML",
        FrameworkKind::GenericNode => "Node.js",
        FrameworkKind::Unknown => "Unknown",
    }
}

pub fn detect_framework(root: &Path) -> FrameworkKind {
    if root.join("next.config.js").exists()
        || root.join("next.config.ts").exists()
        || root.join("next.config.mjs").exists()
    {
        return FrameworkKind::NextJs;
    }
    if root.join("nuxt.config.ts").exists() || root.join("nuxt.config.js").exists() {
        return FrameworkKind::Nuxt;
    }
    if root.join("astro.config.mjs").exists() || root.join("astro.config.ts").exists() {
        return FrameworkKind::Astro;
    }
    if root.join("svelte.config.js").exists() || root.join("svelte.config.ts").exists() {
        return FrameworkKind::SvelteKit;
    }
    if root.join("remix.config.js").exists() || root.join("remix.config.ts").exists() {
        return FrameworkKind::Remix;
    }
    // Check package.json for Remix deps (Remix v2 may not have remix.config)
    if let Some(deps) = read_package_deps(root) {
        if deps.contains("@remix-run/react") || deps.contains("@remix-run/node") {
            return FrameworkKind::Remix;
        }
    }
    if root.join("vite.config.ts").exists()
        || root.join("vite.config.js").exists()
        || root.join("vite.config.mjs").exists()
    {
        return FrameworkKind::Vite;
    }
    if root.join("gatsby-config.js").exists() || root.join("gatsby-config.ts").exists() {
        return FrameworkKind::Gatsby;
    }
    if root.join("angular.json").exists() {
        return FrameworkKind::Angular;
    }
    // Check for Express/Fastify server
    if let Some(deps) = read_package_deps(root) {
        if deps.contains("express") || deps.contains("fastify") {
            return FrameworkKind::Express;
        }
    }
    if root.join("package.json").exists() {
        return FrameworkKind::GenericNode;
    }
    if root.join("index.html").exists() {
        return FrameworkKind::StaticHtml;
    }
    FrameworkKind::Unknown
}

pub fn detect_deploy_mode(root: &Path, user_requested_dynamic: bool) -> DeployMode {
    if !user_requested_dynamic {
        return DeployMode::Static;
    }

    let framework = detect_framework(root);
    match framework {
        FrameworkKind::NextJs => DeployMode::Dynamic(DynamicConfig {
            framework: FrameworkKind::NextJs,
            start_command: vec!["node".into(), ".next/standalone/server.js".into()],
            health_path: "/".into(),
            output_dir: ".next/standalone".into(),
            env_overrides: HashMap::new(),
        }),
        FrameworkKind::Nuxt => {
            let mut env = HashMap::new();
            env.insert("NITRO_PORT".into(), "{PORT}".into());
            DeployMode::Dynamic(DynamicConfig {
                framework: FrameworkKind::Nuxt,
                start_command: vec!["node".into(), ".output/server/index.mjs".into()],
                health_path: "/".into(),
                output_dir: ".output".into(),
                env_overrides: env,
            })
        }
        FrameworkKind::SvelteKit => DeployMode::Dynamic(DynamicConfig {
            framework: FrameworkKind::SvelteKit,
            start_command: vec!["node".into(), "build/index.js".into()],
            health_path: "/".into(),
            output_dir: "build".into(),
            env_overrides: HashMap::new(),
        }),
        FrameworkKind::Remix => DeployMode::Dynamic(DynamicConfig {
            framework: FrameworkKind::Remix,
            start_command: vec!["npx".into(), "remix-serve".into(), "build/server/index.js".into()],
            health_path: "/".into(),
            output_dir: "build".into(),
            env_overrides: HashMap::new(),
        }),
        FrameworkKind::Express | FrameworkKind::GenericNode => DeployMode::Dynamic(DynamicConfig {
            framework: framework.clone(),
            start_command: vec!["npm".into(), "start".into()],
            health_path: "/".into(),
            output_dir: ".".into(),
            env_overrides: HashMap::new(),
        }),
        _ => DeployMode::Static,
    }
}

fn read_package_deps(root: &Path) -> Option<String> {
    let pkg_path = root.join("package.json");
    let content = std::fs::read_to_string(&pkg_path).ok()?;
    Some(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detect_nextjs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("next.config.js"), "module.exports = {}").unwrap();
        assert_eq!(detect_framework(dir.path()), FrameworkKind::NextJs);
    }

    #[test]
    fn detect_nuxt() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("nuxt.config.ts"), "export default {}").unwrap();
        assert_eq!(detect_framework(dir.path()), FrameworkKind::Nuxt);
    }

    #[test]
    fn detect_vite() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("vite.config.ts"), "").unwrap();
        assert_eq!(detect_framework(dir.path()), FrameworkKind::Vite);
    }

    #[test]
    fn detect_static_html() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        assert_eq!(detect_framework(dir.path()), FrameworkKind::StaticHtml);
    }

    #[test]
    fn detect_unknown_empty_dir() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_framework(dir.path()), FrameworkKind::Unknown);
    }

    #[test]
    fn static_mode_when_not_requested() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("next.config.js"), "").unwrap();
        assert!(matches!(detect_deploy_mode(dir.path(), false), DeployMode::Static));
    }

    #[test]
    fn dynamic_mode_nextjs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("next.config.js"), "").unwrap();
        let mode = detect_deploy_mode(dir.path(), true);
        match mode {
            DeployMode::Dynamic(cfg) => {
                assert_eq!(cfg.framework, FrameworkKind::NextJs);
                assert!(cfg.start_command.contains(&"server.js".to_string())
                    || cfg.start_command.iter().any(|s| s.contains("server.js")));
            }
            _ => panic!("expected Dynamic mode"),
        }
    }

    #[test]
    fn display_name() {
        assert_eq!(format!("{}", FrameworkKind::NextJs), "Next.js");
        assert_eq!(format!("{}", FrameworkKind::Vite), "Vite");
    }
}

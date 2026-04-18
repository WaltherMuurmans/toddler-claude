use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FLY_API: &str = "https://api.machines.dev/v1";
const FLY_GQL: &str = "https://api.fly.io/graphql";

pub struct FlyClient {
    http: Client,
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineSpec {
    pub app_name: String,
    pub region: String,
    pub image: String,
    pub cpu_kind: String,
    pub cpus: u32,
    pub memory_mb: u32,
    pub env: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: Option<String>,
    pub state: String,
    #[serde(default)]
    pub private_ip: Option<String>,
}

impl FlyClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest client"),
            token: token.into(),
        }
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    pub async fn verify(&self) -> Result<String> {
        let q = json!({ "query": "query { viewer { email } }" });
        let r: Value = self
            .http
            .post(FLY_GQL)
            .header("Authorization", self.auth())
            .json(&q)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        r["data"]["viewer"]["email"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no viewer email returned; token invalid"))
    }

    pub async fn ensure_app(&self, app_name: &str, org_slug: &str) -> Result<()> {
        let url = format!("{}/apps/{}", FLY_API, app_name);
        let res = self
            .http
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await?;
        if res.status().is_success() {
            return Ok(());
        }
        let create_url = format!("{}/apps", FLY_API);
        let body = json!({
            "app_name": app_name,
            "org_slug": org_slug,
            "network": format!("{}-net", app_name),
        });
        self.http
            .post(&create_url)
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .await?
            .error_for_status()
            .context("create Fly app")?;
        Ok(())
    }

    pub async fn allocate_ipv4(&self, app_name: &str) -> Result<()> {
        let mutation = json!({
            "query": "mutation($appId: ID!) { allocateIpAddress(input:{appId:$appId, type:SHARED_V4}) { ipAddress { address } } }",
            "variables": { "appId": app_name }
        });
        let _ = self
            .http
            .post(FLY_GQL)
            .header("Authorization", self.auth())
            .json(&mutation)
            .send()
            .await?;
        Ok(())
    }

    pub async fn create_machine(&self, spec: &MachineSpec) -> Result<Machine> {
        let url = format!("{}/apps/{}/machines", FLY_API, spec.app_name);
        let body = json!({
            "region": spec.region,
            "config": {
                "image": spec.image,
                "guest": {
                    "cpu_kind": spec.cpu_kind,
                    "cpus": spec.cpus,
                    "memory_mb": spec.memory_mb,
                },
                "env": spec.env,
                "services": [{
                    "protocol": "tcp",
                    "internal_port": 7681,
                    "autostop": "stop",
                    "autostart": false,
                    "min_machines_running": 0,
                    "ports": [
                        { "port": 443, "handlers": ["tls", "http"] }
                    ]
                }],
                "auto_destroy": true,
                "restart": { "policy": "no" }
            }
        });
        let res: Machine = self
            .http
            .post(&url)
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .await?
            .error_for_status()
            .context("create Fly machine")?
            .json()
            .await?;
        Ok(res)
    }

    pub async fn wait_started(&self, app: &str, machine_id: &str, timeout_s: u64) -> Result<()> {
        let url = format!(
            "{}/apps/{}/machines/{}/wait?state=started&timeout={}",
            FLY_API, app, machine_id, timeout_s
        );
        self.http
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await?
            .error_for_status()
            .context("wait started")?;
        Ok(())
    }

    pub async fn destroy_machine(&self, app: &str, machine_id: &str) -> Result<()> {
        let url = format!("{}/apps/{}/machines/{}?force=true", FLY_API, app, machine_id);
        self.http
            .delete(&url)
            .header("Authorization", self.auth())
            .send()
            .await?;
        Ok(())
    }

    pub async fn destroy_app(&self, app: &str) -> Result<()> {
        let url = format!("{}/apps/{}", FLY_API, app);
        self.http
            .delete(&url)
            .header("Authorization", self.auth())
            .send()
            .await?;
        Ok(())
    }
}

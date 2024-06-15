use k8s_openapi::api::networking::v1::{
    HTTPIngressPath,
    HTTPIngressRuleValue,
    Ingress,
    IngressBackend,
    IngressServiceBackend,
    IngressRule,
    IngressSpec,
    ServiceBackendPort,
};
use kube::Api;
use kube::api::{
    ObjectMeta,
    Patch,
    PatchParams,
};
use k8s_openapi::api::core::v1::{
    ConfigMap,
    TypedLocalObjectReference,
};
use kube::{Client, Error};
use std::collections::BTreeMap;
use crate::element::*;
use std::fs;

static NEXTCLOUD_NGINX_MAIN_CONFIG: &str = "files/nextcloud_nginx_ingress_main.conf";
static NEXTCLOUD_NGINX_SERVER_CONFIG: &str = "files/nextcloud_nginx_ingress_server.conf";
static DOCUMENT_ROOT: &str = "/usr/share/nginx/html/";

/// Ingress object functionality
pub trait NextcloudIngress {
    async fn create_ingress(&self, client: Client) -> Result<(), Error>;
}


/// Ingres functionality for the NextcloudElement struct
impl NextcloudIngress for NextcloudElement {
    /// Create an ingress object configured to serve the FastCGI protocol
    async fn create_ingress(&self, client: Client) -> Result<(), Error> {
        let configmap_name = format!("ingress-php-fpm-{}", self.name.clone());
        // Create the ConfigMap object
        let config_map = ConfigMap {
            metadata: ObjectMeta {
                    name: Some(configmap_name.clone()),
                    ..Default::default()
            },
            data: Some(BTreeMap::from([
                (
                    "SCRIPT_FILENAME".to_owned(),
                    format!("{}$fastcgi_script_name", DOCUMENT_ROOT)
                ),
            ])),
            ..Default::default()
        };

        let ingress_rule = IngressRule {
            //TODO: take from the CRD
            host: Some("sotolitolabs.com".to_string()),
            http: Some(HTTPIngressRuleValue {
                paths: vec![
                    HTTPIngressPath {
                        path: Some("/".to_string()),
                        path_type: "Prefix".to_string(),
                        backend: IngressBackend {
                            resource: None,
                            service: Some(IngressServiceBackend {
                                name: "service-nextcloud-php-fpm".to_string(),
                                port: Some(ServiceBackendPort {
                                    name: None,
                                    number: Some(9000),
                                }),
                            }),
                            ..Default::default()
                        },
                    }
                ],
            }),
        };

        let ingress = Ingress {
            metadata: ObjectMeta {
                name: Some(self.name.clone()),
                namespace: Some(self.namespace.clone()),
                labels: Some(self.labels.clone()),
                annotations: Some(BTreeMap::from([
                    (
                        "nginx.ingress.kubernetes.io/backend-protocol".to_owned(),
                        "FCGI".to_owned()
                    ),
                    (
                        "nginx.ingress.kubernetes.io/fastcgi-index".to_owned(),
                        "index.php".to_owned()
                    ),
                    (
                        "nginx.ingress.kubernetes.io/fastcgi-path-info".to_owned(),
                        "true".to_owned()
                    ),

                    (
                        "nginx.ingress.kubernetes.io/fastcgi-params-configmap".to_owned(),
                        configmap_name.clone(),
                    ),
                ])),
                ..Default::default()
            },
            spec: Some(IngressSpec {
                ingress_class_name: Some("nginx".to_string()),
                rules: Some(vec![ingress_rule]),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Create the ConfigMap object
        let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), self.namespace.as_ref());
        let patch_params = PatchParams {
            field_manager: Some("nextcloud_field_manager".to_string()),
            ..PatchParams::default()
        };
        config_maps.patch(
            &configmap_name,
            &patch_params,
            &Patch::Apply(&config_map)
        ).await?;

        // Create the Ingress object
        // NOTE: the ingress controller has to be enabled: minikube addons enable ingress
        // https://kubernetes.github.io/ingress-nginx/deploy/
        let ingresses: Api<Ingress> = Api::namespaced(client.clone(), self.namespace.as_ref());
        let patch_params = PatchParams::apply("nextcloud_field_manger");
        info!("Ingress::Creating or Updating");
        ingresses.patch(
            self.name.as_str(),
            &patch_params,
            &Patch::Apply(&ingress)
        ).await?;
        info!("Ingress::Updated");

        Ok(())
    }
}

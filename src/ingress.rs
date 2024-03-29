use k8s_openapi::api::networking::v1::{
    Ingress,
    IngressBackend,
    IngressRule,
    IngressSpec,
    HTTPIngressPath,
    HTTPIngressRuleValue,
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

/// Ingress object functionality
pub trait NextcloudIngress {
    async fn create_ingress(&self, client: Client) -> Result<(), Error>;
}


/// Ingres functionality for the NextcloudElement struct
impl NextcloudIngress for NextcloudElement {
    /// Create an ingress object configured to serve the FastCGI protocol
    async fn create_ingress(&self, client: Client) -> Result<(), Error> {
        // TODO: check return value
        // Create the ConfigMap object
        let config_map = ConfigMap {
            metadata: ObjectMeta {
                    name: Some(self.name.clone()),
                    //name: Some("nextcloud-ingress".to_string()),
                    ..Default::default()
            },
            data: Some(BTreeMap::from([
                ("DOCUMENT_ROOT".to_owned(), DOCUMENT_ROOT.to_string()),
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
                            resource: Some(TypedLocalObjectReference {
                                api_group: None,
                                kind: "Service".to_string(),
                                name: self.name.clone(),
                            }),
                            //might need service instead
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
                //annotations: Some(self.annotations.clone()),
                annotations: Some(BTreeMap::from([
                    (
                        "nginx.ingress.kubernetes.io/backend-protocol".to_owned(),
                        "FCGI".to_owned()
                    ),
                    (
                        "nginx.ingress.kubernetes.io/fastcgi-path-info".to_owned(),
                        "true".to_owned()
                    ),
                    (
                        "nginx.ingress.kubernetes.io/fastcgi-params-configmap".to_owned(),
                        self.name.clone()
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
            //field_manager: Some("nextcloud_field_manager".to_string()),
            field_manager: Some("nextcloud_field_manager".to_string()),
            ..PatchParams::default()
        };
        config_maps.patch(
            self.name.as_str(),
            &patch_params,
            &Patch::Apply(&config_map)
        ).await?;

        // Create the Ingress object
        // NOTE: the ingress controller has to be enabled: minikube addons enable ingress
        // https://kubernetes.github.io/ingress-nginx/deploy/
        let ingresses: Api<Ingress> = Api::namespaced(client.clone(), self.namespace.as_ref());
        /*let patch_params = PatchParams {
            //field_manager: Some("nextcloud_field_manager".to_string()),
            field_manager: None,
            ..PatchParams::default()
        };*/
        let patch_params = PatchParams::apply("nextcloud_field_manger");
        info!("----- ANTES DE INGRESS");
        ingresses.patch(
            self.name.as_str(),
            &patch_params,
            &Patch::Apply(&ingress)
        ).await?;
        info!("----- DESPUES DE INGRESS");

        Ok(())
    }
}

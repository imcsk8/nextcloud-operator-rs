// Global Constants

// Nextcloud CRD
pub static PREFIX: &str = "nextcloud";
pub static PHP_FPM_NAME: &str = "php-fpm";
pub static PHP_FPM_SERVICE_NAME: &str = "service-nextcloud-php-fpm";
pub static INGRESS_NAME: &str = "ingress";
pub static CONFIGMAP_NAME: &str = "ingress-php-fpm";
pub static FIELD_MANAGER: &str = "nextcloud_field_manager";

// Ingress 
pub static NEXTCLOUD_NGINX_MAIN_CONFIG: &str = "files/nextcloud_nginx_ingress_main.conf";
pub static NEXTCLOUD_NGINX_SERVER_CONFIG: &str = "files/nextcloud_nginx_ingress_server.conf";
pub static DOCUMENT_ROOT: &str = "/usr/share/nginx/html/";



module "glow_with_the_flow" {
  source = "../../"

  vpc = "vpc-0cda00644e0506968"

  ressources = {
    prodapp       = { name = "prod-app", type = "nlb" },
    esengineapp   = { name = "es_engine-app", type = "ec2" },
    esrenderapp   = { name = "es_render-app", type = "ec2" },
    eselasticdata = { name = "es_elastic-data", type = "ec2" },
  }

  flows = {
    prodapp_to_esengineapp_https         = { from = "prodapp", to = "esengineapp", port = 443 },
    esrenderapp_to_esengineapp_httpalt   = { from = "esrenderapp", to = "esengineapp", port = 8080 },
    esengineapp_to_esrenderapp_httpalt   = { from = "esengineapp", to = "esrenderapp", port = 8080 },
    esengineapp_to_eselasticdata_elastic = { from = "esengineapp", to = "eselasticdata", port = 9200 },
    esengineapp_to_eselasticdata_kibana  = { from = "esengineapp", to = "eselasticdata", port = 5601 },
    esengineapp_to_eselasticdata_pgsql   = { from = "esengineapp", to = "eselasticdata", port = 5432 }
  }
}

module "glow_with_the_flow" {
  source = "../../"

  vpc = "vpc-0cda00644e0506968"

  ressources = {
    prodapp     = { name = "prod-app", type = "nlb" },
    esengineapp = { name = "es_engine-app", type = "ec2" }
  }

  flows = {
    prodapp_to_esengineapp_https = { from = "prodapp", to = "esengineapp", port = 443 }
  }
}

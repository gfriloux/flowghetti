module "glow_with_the_flow" {
  source = "../../"

  vpc = "vpc-0cda00644e0506968"

  ressources = {
    prodapp     = { name = "prod-app", type = "nlb" },
    esengineapp = { name = "es_engine-app", type = "ec2" }
  }

  flows = {
    prodapp_to_esengineapp_https = { from = "prodapp", to = "esengineapp", port = 443 },
    random_to_prodapp_https      = { from_cidr = ["1.1.1.1/32", "2.2.2.2/32"], to = "prodapp", port = 443 },
    esengineapp_to_random_https  = { from = "prodapp", to_cidr = ["3.3.3.3/32"], port = 443 }
  }
}

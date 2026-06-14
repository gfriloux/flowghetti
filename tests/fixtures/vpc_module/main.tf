module "glow_with_the_flow" {
  source = "../../"

  # The VPC id comes from a module output whose value is a resource attribute,
  # unknowable by static analysis. The resources must still be grouped — under
  # the origin reference.
  vpc = module.generic_aws_vpc.vpc_id

  ressources = {
    edge = { name = "edge", type = "nlb" },
    app  = { name = "app", type = "ec2" }
  }

  flows = {
    edge_to_app_https = { from = "edge", to = "app", port = 443 }
  }
}

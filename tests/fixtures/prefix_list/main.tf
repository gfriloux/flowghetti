data "aws_ec2_managed_prefix_list" "cloudfront" {
  name = "com.amazonaws.global.cloudfront.origin-facing"
}

module "glow_with_the_flow" {
  source = "../../"

  vpc = "vpc-0cda00644e0506968"

  ressources = {
    prodapp = { name = "prod-app", type = "nlb" }
  }

  flows = {
    cloudfront_to_prodapp_https = { from_prefix_list = [data.aws_ec2_managed_prefix_list.cloudfront.id], to = "prodapp", port = 443 },
  }
}

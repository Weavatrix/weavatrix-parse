use super::extract;
use crate::facts::DeclarationKind;

#[test]
fn blocks_declare_the_objects_the_rest_of_the_file_addresses() {
    let source = "resource \"aws_s3_bucket\" \"logs\" {\n\
         \x20 bucket = \"my-logs\"\n\
         }\n\
         variable \"region\" { default = \"eu-west-1\" }\n\
         data \"aws_ami\" \"ubuntu\" { most_recent = true }\n\
         output \"bucket_arn\" { value = \"x\" }\n";
    let declared = extract(source)
        .declarations
        .into_iter()
        .map(|item| (item.name, item.kind))
        .collect::<Vec<_>>();
    assert_eq!(
        declared,
        [
            ("aws_s3_bucket.logs".to_owned(), DeclarationKind::Resource),
            ("variable.region".to_owned(), DeclarationKind::Variable),
            ("data.aws_ami.ubuntu".to_owned(), DeclarationKind::Resource),
            ("bucket_arn".to_owned(), DeclarationKind::Resource),
        ],
        "a resource is addressed by type and name together"
    );
}

#[test]
fn a_module_names_the_configuration_it_pulls_in() {
    let source = "module \"vpc\" {\n\
         \x20 source  = \"./modules/vpc\"\n\
         \x20 version = \"1.2.0\"\n\
         }\n\
         terraform {\n\
         \x20 required_providers {\n\
         \x20   aws = { source = \"hashicorp/aws\" }\n\
         \x20 }\n\
         }\n";
    let facts = extract(source);
    assert_eq!(
        facts
            .imports
            .iter()
            .map(|import| import.specifier.as_str())
            .collect::<Vec<_>>(),
        ["./modules/vpc", "hashicorp/aws"],
        "a local module and a registry provider are both dependencies"
    );
    assert!(
        facts
            .declarations
            .iter()
            .any(|item| item.name == "module.vpc")
    );
}

#[test]
fn interpolations_reference_the_objects_they_name() {
    let source = "resource \"aws_instance\" \"web\" {\n\
         \x20 ami           = data.aws_ami.ubuntu.id\n\
         \x20 subnet_id     = module.vpc.public_subnet\n\
         \x20 instance_type = var.instance_type\n\
         \x20 bucket        = aws_s3_bucket.logs.arn\n\
         }\n";
    let used = extract(source)
        .references
        .into_iter()
        .map(|reference| (reference.name, reference.owner))
        .collect::<Vec<_>>();
    assert_eq!(
        used,
        [
            (
                "data.aws_ami.ubuntu".to_owned(),
                Some("aws_instance.web".to_owned())
            ),
            ("module.vpc".to_owned(), Some("aws_instance.web".to_owned())),
            (
                "var.instance_type".to_owned(),
                Some("aws_instance.web".to_owned())
            ),
            (
                "aws_s3_bucket.logs".to_owned(),
                Some("aws_instance.web".to_owned())
            ),
        ],
        "every reference belongs to the resource that makes it"
    );
}

#[test]
fn a_comment_declares_nothing() {
    let source = "# resource \"aws_s3_bucket\" \"ghost\" {}\n\
         // module \"ghost\" { source = \"./nowhere\" }\n\
         /* variable \"ghost\" {} */\n\
         resource \"aws_vpc\" \"real\" {}\n";
    let facts = extract(source);
    assert_eq!(facts.declarations.len(), 1);
    assert!(facts.imports.is_empty());
}

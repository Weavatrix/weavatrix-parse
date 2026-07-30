use super::extract;

#[test]
fn a_script_depends_on_what_it_sources_and_what_it_runs() {
    let source = "#!/usr/bin/env bash\n\
         source ./lib/common.sh\n\
         . \"${DIR}/env.sh\"\n\
         bash scripts/migrate.sh --yes\n\
         ./scripts/deploy.sh production\n\
         echo \"source ./ghost.sh\"\n";
    assert_eq!(
        extract(source)
            .imports
            .into_iter()
            .map(|import| import.specifier)
            .collect::<Vec<_>>(),
        [
            "./lib/common.sh",
            "${DIR}/env.sh",
            "scripts/migrate.sh",
            "./scripts/deploy.sh",
        ],
        "a path inside a string argument is text, not a dependency"
    );
}

#[test]
fn a_client_command_records_the_endpoint_it_addresses() {
    let source = "curl -sf http://localhost:8080/api/v1/health\n\
         curl -X POST \"https://api.example.com/v2/jobs\" -d @payload.json\n\
         wget https://cdn.example.com/artifact.tgz\n\
         echo https://not-a-request.example.com\n";
    let addressed = extract(source)
        .references
        .into_iter()
        .filter(|reference| !reference.string_arguments.is_empty())
        .map(|reference| (reference.name, reference.string_arguments))
        .collect::<Vec<_>>();
    assert_eq!(
        addressed,
        [
            (
                "curl".to_owned(),
                vec!["http://localhost:8080/api/v1/health".to_owned()]
            ),
            (
                "curl".to_owned(),
                vec![
                    "POST".to_owned(),
                    "https://api.example.com/v2/jobs".to_owned()
                ]
            ),
            (
                "wget".to_owned(),
                vec!["https://cdn.example.com/artifact.tgz".to_owned()]
            ),
        ],
        "echo is not a client, so its argument is not an endpoint"
    );
}

#[test]
fn functions_are_declared_and_own_the_commands_inside_them() {
    let source = "deploy() {\n\
         \x20 curl -sf http://svc/ready\n\
         }\n\
         \n\
         function rollback {\n\
         \x20 kubectl rollout undo\n\
         }\n\
         \n\
         deploy\n";
    let facts = extract(source);
    assert_eq!(
        facts
            .declarations
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["deploy", "rollback"],
        "both spellings of a definition count"
    );
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.name == "curl"
                && reference.owner.as_deref() == Some("deploy")),
        "a command belongs to the function it is written in"
    );
    assert!(
        facts
            .references
            .iter()
            .any(|reference| reference.name == "kubectl"
                && reference.owner.as_deref() == Some("rollback")),
        "and the next function owns its own"
    );
}

#[test]
fn a_comment_is_not_a_command_and_a_hash_in_a_string_is_not_a_comment() {
    let source = "# curl http://ghost/api\n\
         echo \"a # inside a string\" && curl http://real/api\n";
    let names = extract(source)
        .references
        .into_iter()
        .map(|reference| reference.name)
        .collect::<Vec<_>>();
    assert!(names.contains(&"curl".to_owned()), "got {names:?}");
    assert_eq!(
        names.iter().filter(|name| *name == "curl").count(),
        1,
        "only the command after the string, got {names:?}"
    );
}

#[test]
fn an_argument_is_not_read_as_a_command_of_its_own() {
    let source = "docker run --rm -v /tmp:/tmp alpine sh -c 'echo hi'\n";
    let names = extract(source)
        .references
        .into_iter()
        .map(|reference| reference.name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["docker"],
        "everything after the command word is an argument, got {names:?}"
    );
}

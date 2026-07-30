use super::extract;
use crate::facts::DeclarationKind;

fn specifiers(source: &str) -> Vec<String> {
    extract(source)
        .imports
        .into_iter()
        .map(|import| import.specifier)
        .collect()
}

#[test]
fn created_objects_carry_their_kind() {
    let source = "CREATE TABLE app.users (id INT);\n\
         create or replace view active_users as select 1;\n\
         CREATE OR REPLACE FUNCTION bump() RETURNS INT AS 'select 1';\n";
    let declared = extract(source)
        .declarations
        .into_iter()
        .map(|item| (item.name, item.kind))
        .collect::<Vec<_>>();
    assert_eq!(
        declared,
        [
            ("app.users".to_owned(), DeclarationKind::Table),
            ("active_users".to_owned(), DeclarationKind::View),
            ("bump".to_owned(), DeclarationKind::Function),
        ],
        "keywords are matched whichever case they are written in"
    );
}

#[test]
fn a_view_depends_on_every_table_it_reads() {
    let source = "CREATE VIEW report AS\n\
         SELECT o.id\n\
         FROM orders o\n\
         JOIN app.customers c ON c.id = o.customer_id\n\
         LEFT JOIN payments ON payments.order_id = o.id;\n";
    assert_eq!(
        specifiers(source),
        ["orders", "app.customers", "payments"],
        "an ON inside a join is a condition, not another table"
    );
}

#[test]
fn writes_and_alterations_are_dependencies_too() {
    let source = "INSERT INTO events (id) VALUES (1);\n\
         UPDATE accounts SET balance = 0;\n\
         ALTER TABLE accounts ADD COLUMN note TEXT;\n\
         CREATE INDEX idx_events_id ON events (id);\n";
    assert_eq!(
        specifiers(source),
        ["events", "accounts", "accounts", "events"],
        "an ON after CREATE INDEX names the indexed table"
    );
}

#[test]
fn a_comment_or_string_never_declares_a_table() {
    let source = "-- CREATE TABLE ghost (id INT);\n\
         /* CREATE TABLE also_ghost (id INT); */\n\
         INSERT INTO log (message) VALUES ('select * from phantom');\n\
         CREATE TABLE real_one (id INT);\n";
    let facts = extract(source);
    assert_eq!(
        facts
            .declarations
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["real_one"]
    );
    assert_eq!(
        specifiers(source),
        ["log"],
        "the table named inside the string literal is text"
    );
}

#[test]
fn a_comma_separated_from_list_names_every_table() {
    assert_eq!(
        specifiers("SELECT * FROM users u, orders o, app.items;"),
        ["users", "orders", "app.items"],
        "an alias is not a table"
    );
}

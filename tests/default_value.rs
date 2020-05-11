use async_graphql::prelude::*;
use async_graphql::{EmptyMutation, EmptySubscription};

#[async_std::test]
pub async fn test_default_value_arg() {
    struct Query;

    #[GqlObject]
    impl Query {
        async fn value(&self, #[arg(default = "100")] input: i32) -> i32 {
            input
        }
    }

    let query = "{ value }";
    let schema = GqlSchema::new(Query, EmptyMutation, EmptySubscription);
    assert_eq!(
        schema.execute(&query).await.unwrap().data,
        serde_json::json!({
            "value": 100
        })
    );

    let query = "{ value(input: 88) }";
    let schema = GqlSchema::new(Query, EmptyMutation, EmptySubscription);
    assert_eq!(
        schema.execute(&query).await.unwrap().data,
        serde_json::json!({
            "value": 88
        })
    );
}

#[async_std::test]
pub async fn test_default_value_inputobject() {
    #[GqlInputObject]
    struct MyInput {
        #[field(default = "100")]
        value: i32,
    }

    struct Query;

    #[GqlObject]
    impl Query {
        async fn value(&self, input: MyInput) -> i32 {
            input.value
        }
    }

    let query = "{ value(input: {}) }";
    let schema = GqlSchema::new(Query, EmptyMutation, EmptySubscription);
    assert_eq!(
        schema.execute(&query).await.unwrap().data,
        serde_json::json!({
            "value": 100
        })
    );

    let query = "{ value(input: { value: 88 }) }";
    let schema = GqlSchema::new(Query, EmptyMutation, EmptySubscription);
    assert_eq!(
        schema.execute(&query).await.unwrap().data,
        serde_json::json!({
            "value": 88
        })
    );
}

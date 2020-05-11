//! Async-graphql integration with Wrap

#![warn(missing_docs)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_doctest_main)]

use async_graphql::http::StreamBody;
use async_graphql::{
    GqlData, GqlFieldResult, GqlQueryBuilder, GqlSchema, IntoGqlQueryBuilder,
    IntoGqlQueryBuilderOpts, ObjectType, SubscriptionType, WebSocketTransport,
};
use bytes::Bytes;
use futures::select;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use warp::filters::ws::Message;
use warp::filters::BoxedFilter;
use warp::reject::Reject;
use warp::{Filter, Rejection, Reply};

/// Bad request error
///
/// It's a wrapper of `async_graphql::ParseRequestError`.
pub struct BadRequest(pub async_graphql::ParseRequestError);

impl std::fmt::Debug for BadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Reject for BadRequest {}

/// GraphQL request filter
///
/// It outputs a tuple containing the `GqlSchema` and `QuertBuilder`.
///
/// # Examples
/// *[Full Example](<https://github.com/async-graphql/examples/blob/master/warp/starwars/src/main.rs>)*
///
/// ```no_run
///
/// use async_graphql::prelude::*;
/// use async_graphql::{EmptyMutation, EmptySubscription};
/// use warp::{Filter, Reply};
/// use std::convert::Infallible;
/// use async_graphql::http::GQLResponse;
///
/// struct QueryRoot;
///
/// #[GqlObject]
/// impl QueryRoot {
///     #[field]
///     async fn value(&self, ctx: &GqlContext<'_>) -> i32 {
///         unimplemented!()
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let schema = GqlSchema::new(QueryRoot, EmptyMutation, EmptySubscription);
///     let filter = async_graphql_warp::graphql(schema).and_then(|(schema, builder): (_, GqlQueryBuilder)| async move {
///         let resp = builder.execute(&schema).await;
///         Ok::<_, Infallible>(warp::reply::json(&GQLResponse(resp)).into_response())
///     });
///     warp::serve(filter).run(([0, 0, 0, 0], 8000)).await;
/// }
/// ```
pub fn graphql<Query, Mutation, Subscription>(
    schema: GqlSchema<Query, Mutation, Subscription>,
) -> BoxedFilter<((GqlSchema<Query, Mutation, Subscription>, GqlQueryBuilder),)>
where
    Query: ObjectType + Send + Sync + 'static,
    Mutation: ObjectType + Send + Sync + 'static,
    Subscription: SubscriptionType + Send + Sync + 'static,
{
    warp::any()
        .and(warp::post())
        .and(warp::header::optional::<String>("content-type"))
        .and(warp::body::stream())
        .and(warp::any().map(move || schema.clone()))
        .and_then(|content_type, body, schema| async move {
            let builder = (content_type, StreamBody::new(body))
                .into_query_builder()
                .await
                .map_err(|err| warp::reject::custom(BadRequest(err)))?;
            Ok::<_, Rejection>((schema, builder))
        })
        .boxed()
}

/// Similar to graphql, but you can set the options `IntoGqlQueryBuilderOpts`.
pub fn graphql_opts<Query, Mutation, Subscription>(
    schema: GqlSchema<Query, Mutation, Subscription>,
    opts: IntoGqlQueryBuilderOpts,
) -> BoxedFilter<((GqlSchema<Query, Mutation, Subscription>, GqlQueryBuilder),)>
where
    Query: ObjectType + Send + Sync + 'static,
    Mutation: ObjectType + Send + Sync + 'static,
    Subscription: SubscriptionType + Send + Sync + 'static,
{
    let opts = Arc::new(opts);
    warp::any()
        .and(warp::post())
        .and(warp::header::optional::<String>("content-type"))
        .and(warp::body::stream())
        .and(warp::any().map(move || opts.clone()))
        .and(warp::any().map(move || schema.clone()))
        .and_then(
            |content_type, body, opts: Arc<IntoGqlQueryBuilderOpts>, schema| async move {
                let builder = (content_type, StreamBody::new(body))
                    .into_query_builder_opts(&opts)
                    .await
                    .map_err(|err| warp::reject::custom(BadRequest(err)))?;
                Ok::<_, Rejection>((schema, builder))
            },
        )
        .boxed()
}

/// GraphQL subscription filter
///
/// # Examples
///
/// ```no_run
/// use async_graphql::prelude::*;
/// use async_graphql::EmptyMutation;
/// use warp::Filter;
/// use futures::{Stream, StreamExt};
/// use std::time::Duration;
///
/// struct QueryRoot;
///
/// #[GqlObject]
/// impl QueryRoot {}
///
/// struct SubscriptionRoot;
///
/// #[GqlSubscription]
/// impl SubscriptionRoot {
///     #[field]
///     async fn tick(&self) -> impl Stream<Item = String> {
///         tokio::time::interval(Duration::from_secs(1)).map(|n| format!("{}", n.elapsed().as_secs_f32()))
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let schema = GqlSchema::new(QueryRoot, EmptyMutation, SubscriptionRoot);
///     let filter = async_graphql_warp::graphql_subscription(schema);
///     warp::serve(filter).run(([0, 0, 0, 0], 8000)).await;
/// }
/// ```
pub fn graphql_subscription<Query, Mutation, Subscription>(
    schema: GqlSchema<Query, Mutation, Subscription>,
) -> BoxedFilter<(impl Reply,)>
where
    Query: ObjectType + Sync + Send + 'static,
    Mutation: ObjectType + Sync + Send + 'static,
    Subscription: SubscriptionType + Send + Sync + 'static,
{
    warp::any()
        .and(warp::ws())
        .and(warp::any().map(move || schema.clone()))
        .map(
            |ws: warp::ws::Ws, schema: GqlSchema<Query, Mutation, Subscription>| {
                ws.on_upgrade(move |websocket| {
                    let (mut tx, rx) = websocket.split();
                    let (mut stx, srx) =
                        schema.subscription_connection(WebSocketTransport::default());

                    let mut rx = rx.fuse();
                    let mut srx = srx.fuse();

                    async move {
                        loop {
                            select! {
                                bytes = srx.next() => {
                                    if let Some(bytes) = bytes {
                                        if tx
                                            .send(Message::text(unsafe {
                                                String::from_utf8_unchecked(bytes.to_vec())
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    } else {
                                        return;
                                    }
                                }
                                msg = rx.next() => {
                                    if let Some(Ok(msg)) = msg {
                                        if msg.is_text() {
                                            if stx.send(Bytes::copy_from_slice(msg.as_bytes())).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                })
            },
        ).map(|reply| {
            warp::reply::with_header(reply, "Sec-WebSocket-Protocol", "graphql-ws")
        })
        .boxed()
}

/// GraphQL subscription filter
///
/// Specifies that a function converts the init payload to data.
pub fn graphql_subscription_with_data<Query, Mutation, Subscription, F>(
    schema: GqlSchema<Query, Mutation, Subscription>,
    init_context_data: F,
) -> BoxedFilter<(impl Reply,)>
where
    Query: ObjectType + Sync + Send + 'static,
    Mutation: ObjectType + Sync + Send + 'static,
    Subscription: SubscriptionType + Send + Sync + 'static,
    F: Fn(serde_json::Value) -> GqlFieldResult<GqlData> + Send + Sync + Clone + 'static,
{
    warp::any()
        .and(warp::ws())
        .and(warp::any().map(move || schema.clone()))
        .and(warp::any().map(move || init_context_data.clone()))
        .map(
            |ws: warp::ws::Ws, schema: GqlSchema<Query, Mutation, Subscription>, init_context_data: F| {
                ws.on_upgrade(move |websocket| {
                    let (mut tx, rx) = websocket.split();
                    let (mut stx, srx) =
                        schema.subscription_connection(WebSocketTransport::new(init_context_data));

                    let mut rx = rx.fuse();
                    let mut srx = srx.fuse();

                    async move {
                        loop {
                            select! {
                                bytes = srx.next() => {
                                    if let Some(bytes) = bytes {
                                        if tx
                                            .send(Message::text(unsafe {
                                                String::from_utf8_unchecked(bytes.to_vec())
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    } else {
                                        return;
                                    }
                                }
                                msg = rx.next() => {
                                    if let Some(Ok(msg)) = msg {
                                        if msg.is_text() {
                                            if stx.send(Bytes::copy_from_slice(msg.as_bytes())).await.is_err() {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                })
            },
        ).map(|reply| {
        warp::reply::with_header(reply, "Sec-WebSocket-Protocol", "graphql-ws")
    })
        .boxed()
}

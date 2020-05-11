use crate::http::multipart::{Multipart, PartData};
use crate::http::GQLRequest;
use crate::query::{IntoGqlQueryBuilder, IntoGqlQueryBuilderOpts};
use crate::{GqlQueryBuilder, ParseRequestError};
use futures::{AsyncRead, AsyncReadExt};
use mime::Mime;
use std::collections::HashMap;

#[async_trait::async_trait]
impl<CT, Body> IntoGqlQueryBuilder for (Option<CT>, Body)
where
    CT: AsRef<str> + Send,
    Body: AsyncRead + Send + Unpin,
{
    async fn into_query_builder_opts(
        mut self,
        opts: &IntoGqlQueryBuilderOpts,
    ) -> std::result::Result<GqlQueryBuilder, ParseRequestError> {
        if let Some(boundary) = self
            .0
            .and_then(|value| value.as_ref().parse::<Mime>().ok())
            .and_then(|ct| {
                if ct.essence_str() == mime::MULTIPART_FORM_DATA {
                    ct.get_param("boundary")
                        .map(|boundary| boundary.to_string())
                } else {
                    None
                }
            })
        {
            // multipart
            let mut multipart = Multipart::parse(
                self.1,
                boundary.as_str(),
                opts.max_file_size,
                opts.max_num_files,
            )
            .await?;
            let gql_request: GQLRequest = {
                let part = multipart
                    .remove("operations")
                    .ok_or_else(|| ParseRequestError::MissingOperatorsPart)?;
                let reader = part.create_reader()?;
                serde_json::from_reader(reader).map_err(ParseRequestError::InvalidRequest)?
            };
            let mut map: HashMap<String, Vec<String>> = {
                let part = multipart
                    .remove("map")
                    .ok_or_else(|| ParseRequestError::MissingMapPart)?;
                let reader = part.create_reader()?;
                serde_json::from_reader(reader).map_err(ParseRequestError::InvalidFilesMap)?
            };

            let mut builder = gql_request.into_query_builder().await?;

            // read files
            for part in &multipart.parts {
                if let Some(name) = &part.name {
                    if let Some(var_paths) = map.remove(name) {
                        for var_path in var_paths {
                            if let (Some(filename), PartData::File(content)) =
                                (&part.filename, &part.data)
                            {
                                builder.set_upload(
                                    &var_path,
                                    filename.clone(),
                                    part.content_type.clone(),
                                    content.try_clone().unwrap(),
                                );
                            }
                        }
                    }
                }
            }

            if !map.is_empty() {
                return Err(ParseRequestError::MissingFiles);
            }

            Ok(builder)
        } else {
            let mut data = Vec::new();
            self.1
                .read_to_end(&mut data)
                .await
                .map_err(ParseRequestError::Io)?;
            let gql_request: GQLRequest =
                serde_json::from_slice(&data).map_err(ParseRequestError::InvalidRequest)?;
            gql_request.into_query_builder().await
        }
    }
}

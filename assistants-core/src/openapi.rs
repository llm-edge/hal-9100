use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionSignature {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActionRequest {
    pub domain: String,
    pub path: String,
    pub method: String,
    pub operation: String,
    pub operation_hash: Option<String>,
    pub is_consequential: bool,
    pub content_type: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenAPISpec {
    pub openapi_spec: oas3::OpenApiV3Spec,
}

impl OpenAPISpec {
    pub fn new(spec_str: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let openapi_spec = oas3::from_reader(spec_str.as_bytes())?;
        Ok(Self { openapi_spec })
    }
    pub fn get_functions(&self) -> Result<Vec<oas3::spec::Operation>, serde_json::Error> {
        let mut operations = Vec::new();
        for methods in self.openapi_spec.paths.values() {
            for (_, operation) in methods.methods() {
                operations.push(operation.clone());
            }
        }
        Ok(operations)
    }

    pub fn get_http_requests(&self) -> HashMap<String, ActionRequest> {
        let mut requests = HashMap::new();

        for (path, methods) in &self.openapi_spec.paths {
            for (method, spec) in methods.methods() {
                let request = ActionRequest {
                    domain: self.openapi_spec.servers[0].url.clone(),
                    path: path.to_string(),
                    method: method.to_string(),
                    operation: spec.operation_id.as_ref().unwrap().to_string(),
                    operation_hash: None,
                    is_consequential: false,
                    content_type: "application/json".to_string(),
                    params: None,
                };
                requests.insert(request.operation.clone(), request);
            }
        }

        requests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_data::OPENAPI_SPEC;
    use oas3;
    #[test]
    fn test_get_functions_and_requests() {
        // Read the OpenAPI spec from a file
        let openapi_spec = oas3::from_reader(OPENAPI_SPEC.as_bytes()).unwrap();
        let openapi = OpenAPISpec { openapi_spec };

        // Test get_functions
        let functions = openapi.get_functions().unwrap();
        assert!(!functions.is_empty());

        // Test get_http_requests
        let requests = openapi.get_http_requests();
        assert!(!requests.is_empty());
    }
}

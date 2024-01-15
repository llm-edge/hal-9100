pub static OPENAPI_SPEC: &str = r#"
openapi: 3.0.0
info:
title: MediaWiki Random API
description: This API returns a set of random pages from MediaWiki.
version: 1.0.0
servers:
- url: https://en.wikipedia.org/w
    description: Wikipedia API Server
paths:
/api.php:
    get:
    operationId: getRandomPages
    summary: Get a set of random pages
    description: Returns a list of random pages from MediaWiki.
    parameters:
        - name: action
        in: query
        required: true
        description: The action to perform.
        schema:
            type: string
            default: query
        - name: format
        in: query
        required: true
        description: The format of the output.
        schema:
            type: string
            default: json
        - name: list
        in: query
        required: true
        description: Specify the list as random.
        schema:
            type: string
            default: random
        - name: rnnamespace
        in: query
        required: false
        description: Return pages in these namespaces only.
        schema:
            type: string
        - name: rnfilterredir
        in: query
        required: false
        description: How to filter for redirects.
        schema:
            type: string
            enum: [all, nonredirects, redirects]
            default: nonredirects
        - name: rnlimit
        in: query
        required: false
        description: Limit how many random pages will be returned.
        schema:
            type: integer
            default: 1
            minimum: 1
            maximum: 500
    responses:
        '200':
        description: A list of random pages
        content:
            application/json:
            schema: 
                type: object
                properties:
                batchcomplete:
                    type: string
                continue:
                    type: object
                    properties:
                    rncontinue:
                        type: string
                    continue:
                        type: string
                query:
                    type: object
                    properties:
                    random:
                        type: array
                        items:
                        type: object
                        properties:
                            id:
                            type: integer
                            ns:
                            type: integer
                            title:
                            type: string
"#;

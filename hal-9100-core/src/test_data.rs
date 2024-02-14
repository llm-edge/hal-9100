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


// TODO: no idea how to do full text search thru api https://postgrest.org/en/stable/references/api/tables_views.html#full-text-search

pub static OPENAPI_SPEC_SUPABASE_API: &str = r#"
openapi: 3.0.0
info:
  title: Supabase Schedules API
  version: 1.0.0
  description: API for querying schedules from Supabase.
servers:
  - url: https://api.supabase.io
    description: Supabase API Server
paths:
  /schedules:
    get:
      operationId: getSchedules
      summary: |
        Get schedules with optional filters. 
        Examples:
          1. Get schedules with description that start with O or P: description=like(any).{O*,P*})
          2. Get schedules at specific time: start_at=gte.2021-01-01&end_at=lte.2021-12-31. Keep narrow time range to avoid fetching too many schedules.
      description: Fetch schedules with optional filters.
      parameters:
        - in: query
          name: start_at
          schema:
            type: string
            format: date-time
          description: Filter schedules starting from this timestamp. (lt, gt, eq, etc.)
        - in: query
          name: end_at
          schema:
            type: string
            format: date-time
          description: Filter schedules ending by this timestamp.
        - in: query
          name: title
          schema:
            type: string
          description: Filter schedules by title.
        - in: query
          name: description
          schema:
            type: string
          description: Filter schedules by description.
      responses:
        '200':
          description: A list of schedules.
          content:
            application/json:
              schema:
                type: object
                properties:
                  data:
                    type: array
                    items:
                      $ref: '#/components/schemas/Schedule'
                  error:
                    $ref: '#/components/schemas/Error'
        '400':
          description: Bad request.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/Error'
components:
  schemas:
    Schedule:
      type: object
      properties:
        id:
          type: integer
        created_at:
          type: string
          format: date-time
        start_at:
          type: string
          format: date-time
        end_at:
          type: string
          format: date-time
        user_id:
          type: string
          format: uuid
        title:
          type: string
        description:
          type: string
    Error:
      type: object
      properties:
        message:
          type: string
"#;

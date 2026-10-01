import type { WorkspaceFile } from './types';

export interface Example {
  id: string;
  title: string;
  config: string;
  files: WorkspaceFile[];
}

const USERS = `openapi: 3.0.3
info:
  title: Users API
  version: 1.0.0
tags:
  - name: users
paths:
  /users:
    get:
      tags: [users]
      summary: List users
      responses:
        '200':
          description: Users
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/User'
components:
  schemas:
    User:
      type: object
      properties:
        id: { type: integer }
        email: { type: string }
    Error:
      type: object
      properties:
        message: { type: string }
`;

const ORDERS = `openapi: 3.0.3
info:
  title: Orders API
  version: 1.0.0
tags:
  - name: orders
paths:
  /orders:
    get:
      tags: [orders]
      summary: List orders
      responses:
        '200':
          description: Orders
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Order'
components:
  schemas:
    Order:
      type: object
      properties:
        id: { type: integer }
        total: { type: number }
    Error:
      type: object
      properties:
        message: { type: string }
`;

const USERS_READ = `openapi: 3.0.3
info:
  title: Users (read)
  version: 1.0.0
paths:
  /users:
    get:
      summary: List users
      responses:
        '200': { description: Users }
`;

const USERS_WRITE = `openapi: 3.0.3
info:
  title: Users (write)
  version: 1.0.0
paths:
  /users:
    post:
      summary: Create a user
      responses:
        '201': { description: Created }
`;

const INVENTORY = `openapi: 3.0.3
info:
  title: Inventory API
  version: 1.0.0
paths:
  /items:
    get:
      summary: List stock items
      responses:
        '200':
          description: Items
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Item'
components:
  schemas:
    Item:
      type: object
      properties:
        sku: { type: string }
        quantity: { type: integer }
`;

const CATALOG = `openapi: 3.0.3
info:
  title: Catalog API
  version: 1.0.0
paths:
  /items:
    get:
      summary: List catalog items
      responses:
        '200':
          description: Items
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Item'
components:
  schemas:
    Item:
      type: object
      properties:
        id: { type: integer }
        title: { type: string }
`;

export const EXAMPLES: Example[] = [
  {
    id: 'rename',
    title: 'Conflicts → rename',
    config: `# Both services define GET /items and a different "Item" schema.
# With rename, the second source's path and schema get its name as a prefix,
# and its $refs are rewritten to match.
sources:
  - name: inventory
    path: ./specs/inventory.yaml
  - name: catalog
    path: ./specs/catalog.yaml

merge:
  conflict_strategy: rename
  info:
    title: Shop API
    version: 1.0.0
`,
    files: [
      { name: 'specs/inventory.yaml', content: INVENTORY },
      { name: 'specs/catalog.yaml', content: CATALOG },
    ],
  },
  {
    id: 'clean',
    title: 'Clean merge of two services',
    config: `# No conflicts: paths and schemas are combined.
# Both define an identical "Error" schema, which is shared, not duplicated.
sources:
  - name: users
    path: ./specs/users.yaml
  - name: orders
    path: ./specs/orders.yaml
`,
    files: [
      { name: 'specs/users.yaml', content: USERS },
      { name: 'specs/orders.yaml', content: ORDERS },
    ],
  },
  {
    id: 'per-operation',
    title: 'Same path, different methods',
    config: `# GET /users and POST /users come from different services.
# They are merged into one path; only the same method twice would conflict.
sources:
  - name: users-read
    path: ./specs/users-read.yaml
  - name: users-write
    path: ./specs/users-write.yaml
`,
    files: [
      { name: 'specs/users-read.yaml', content: USERS_READ },
      { name: 'specs/users-write.yaml', content: USERS_WRITE },
    ],
  },
  {
    id: 'tags',
    title: 'Tag prefixing',
    config: `# Every tag (and every operation's tag reference) is prefixed with its source.
sources:
  - name: users
    path: ./specs/users.yaml
  - name: orders
    path: ./specs/orders.yaml
    tag_prefix: Orders   # custom prefix instead of the source name

merge:
  tag_prefix: source_name
  tag_separator: " / "
`,
    files: [
      { name: 'specs/users.yaml', content: USERS },
      { name: 'specs/orders.yaml', content: ORDERS },
    ],
  },
  {
    id: 'additional-blocks',
    title: 'Additional blocks (vendor extensions)',
    config: `# additional_blocks is deep-merged into a source before merging,
# e.g. to add API gateway extensions without editing the original spec.
sources:
  - name: users
    path: ./specs/users.yaml
    additional_blocks:
      x-gateway:
        upstream: http://users.internal:8080
      paths:
        /users:
          get:
            x-rate-limit: 100

output:
  format: json
`,
    files: [{ name: 'specs/users.yaml', content: USERS }],
  },
  {
    id: 'remote',
    title: 'Remote URL + local file',
    config: `# The Petstore spec is fetched from the web (its server allows CORS).
# Both specs define "User", so rename keeps them apart.
sources:
  - name: petstore
    url: https://petstore3.swagger.io/api/v3/openapi.json
  - name: users
    path: ./specs/users.yaml

merge:
  conflict_strategy: rename
`,
    files: [{ name: 'specs/users.yaml', content: USERS }],
  },
];

export const DEFAULT_EXAMPLE_ID = 'rename';

export function findExample(id: string): Example {
  return EXAMPLES.find((e) => e.id === id) ?? EXAMPLES.find((e) => e.id === DEFAULT_EXAMPLE_ID)!;
}

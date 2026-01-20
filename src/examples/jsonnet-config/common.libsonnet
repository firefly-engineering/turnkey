// Common Jsonnet library with reusable functions

{
  // Database configurations by environment
  local dbConfigs = {
    development: {
      host: 'localhost',
      port: 5432,
      name: 'myapp_dev',
      pool_size: 5,
    },
    staging: {
      host: 'staging-db.example.com',
      port: 5432,
      name: 'myapp_staging',
      pool_size: 10,
    },
    production: {
      host: 'prod-db.example.com',
      port: 5432,
      name: 'myapp_prod',
      pool_size: 20,
    },
  },

  // Get database config for environment
  database(env):: dbConfigs[env],

  // Helper to create a service definition
  service(name, port):: {
    name: name,
    port: port,
    health_check: '/health',
  },
}

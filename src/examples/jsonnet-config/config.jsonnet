// Example Jsonnet configuration file
// Demonstrates variables, functions, and external variables

// Import the common library
local common = import 'common.libsonnet';

// Get environment from external variable (defaults to 'development')
local env = std.extVar('env');

// Configuration object
{
  name: 'my-app',
  version: '1.0.0',
  environment: env,

  // Use function from common library
  database: common.database(env),

  // Conditional configuration based on environment
  logging: {
    level: if env == 'production' then 'warn' else 'debug',
    format: 'json',
  },

  // Array of enabled features
  features: [
    'auth',
    'api',
  ] + if env == 'development' then ['debug-toolbar'] else [],
}

// Test file for common.libsonnet
// Uses std.assertEqual to verify function behavior

local common = import 'common.libsonnet';

// Test database function for development
local devDb = common.database('development');
std.assertEqual(devDb.host, 'localhost') &&
std.assertEqual(devDb.port, 5432) &&
std.assertEqual(devDb.name, 'myapp_dev') &&

// Test database function for production
local prodDb = common.database('production');
std.assertEqual(prodDb.host, 'prod-db.example.com') &&
std.assertEqual(prodDb.port, 5432) &&
std.assertEqual(prodDb.name, 'myapp_prod') &&

// All tests passed
true

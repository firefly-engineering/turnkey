// Example demonstrating external npm package usage in Buck2
//
// This example uses the `chalk` package for colored console output.
// To make this work with Buck2, you need npm/worker integration.

import chalk from 'chalk';

const name = process.argv[2] || 'World';
console.log(chalk.green(`Hello, ${name}!`));

// TypeScript example that uses lodash npm package
import * as _ from "lodash";

const numbers = [1, 2, 3, 4, 5];
const doubled = _.map(numbers, (n: number) => n * 2);
const sum = _.sum(doubled);

console.log(`Original: [${numbers.join(", ")}]`);
console.log(`Doubled: [${doubled.join(", ")}]`);
console.log(`Sum of doubled: ${sum}`);

// Use some lodash utilities
const names = ["Alice", "Bob", "Charlie"];
const greeting = _.join(
	_.map(names, (name: string) => `Hello, ${name}!`),
	"\n",
);
console.log(`\n${greeting}`);

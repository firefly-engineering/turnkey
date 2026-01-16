// Main entry point

import { greet, greetWorld } from "./greeter";

function main(): void {
  console.log(greetWorld());
  console.log(greet("TypeScript"));
  console.log(greet("Buck2"));
}

main();

// A simple greeter module

export function greet(name: string): string {
  return `Hello, ${name}!`;
}

export function greetWorld(): string {
  return greet("World");
}

// Greeter library for multi-language demo

export function greet(name: string): string {
	return `TypeScript: Hello, ${name}!`;
}

export function greetWorld(): string {
	return greet("World");
}

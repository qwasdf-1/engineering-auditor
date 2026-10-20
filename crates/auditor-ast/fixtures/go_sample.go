package main

import "fmt"

func add(a, b int) int {
	return a + b
}

func greet(name string) string {
	return fmt.Sprintf("Hello, %s!", name)
}

type Calculator struct{}

func (c Calculator) Multiply(a, b int) int {
	return a * b
}

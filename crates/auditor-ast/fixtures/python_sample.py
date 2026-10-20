# A sample Python file
def greet(name):
    return f"Hello, {name}!"

def add(a, b):
    return a + b

class Calculator:
    def multiply(self, a, b):
        return a * b

    def divide(self, a, b):
        if b == 0:
            raise ValueError("Cannot divide by zero")
        return a / b

from venice_cli._core import call
import sys

def main():
    print("This is printing from Python code! Now calling the Rust code...")
    result = call(sys.argv)

    print(f"Back in Python! Result: {result}")


if __name__ == "__main__":
    main()

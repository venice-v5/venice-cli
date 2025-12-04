from venice_cli._core import call
import sys

def main():
    # Try to import venice package to get runtime binary path and version
    binary_path = None
    version = None
    
    try:
        import venice
        binary_path = venice._dangerous_DO_NOT_TOUCH_YOU_WILL_GET_ELECTROCUTED_get_binary_path()
        version = venice._dangerous_DO_NOT_TOUCH_YOU_WILL_GET_ELECTROCUTED_get_version()
    except (ImportError, AttributeError):
        # venice package not installed or doesn't have the required functions
        pass
    
    result = call(sys.argv, binary_path, version)


if __name__ == "__main__":
    main()

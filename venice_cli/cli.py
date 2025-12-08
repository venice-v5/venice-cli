from venice_cli._core import call
import sys
import mpy_cross

def main():
    binary_path = None
    version = None

    try:
        import venice  # pyright: ignore[reportMissingImports]
        binary_path = venice._dangerous_DO_NOT_TOUCH_YOU_WILL_GET_ELECTROCUTED_get_binary_path()
        version = venice._dangerous_DO_NOT_TOUCH_YOU_WILL_GET_ELECTROCUTED_get_version()
    except (ImportError, AttributeError):
        pass

    call(sys.argv, str(binary_path), version, mpy_cross.mpy_cross)


if __name__ == "__main__":
    main()

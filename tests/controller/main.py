import venice

cont = venice.Controller()
try:
    print(cont.read_state())
except BaseException as e:
    print("oop", e)

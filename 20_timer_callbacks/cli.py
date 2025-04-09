import subprocess
import readline  # For command history and editing
import os

TTY_DEVICE = "/dev/ttyUSB0"

def send_command_to_tty(command: str):
    if not command.strip():
        return
    try:
        full_command = f'echo "{command}" > {TTY_DEVICE}'
        subprocess.run(['sudo', 'bash', '-c', full_command], check=True)
        print(f"✅ Sent: {command}")
    except subprocess.CalledProcessError as e:
        print(f"❌ Error sending command: {e}")

def main():
    print(f"📟 TTY Command Sender - Redirecting to {TTY_DEVICE}")
    print("Type your commands. Type 'exit' or Ctrl+C to quit.\n")

    try:
        while True:
            try:
                cmd = input("🧾 Command> ")
                if cmd.lower() in ('exit', 'quit'):
                    print("👋 Exiting...")
                    break
                send_command_to_tty(cmd)
            except EOFError:
                print("\n👋 Exiting...")
                break
    except KeyboardInterrupt:
        print("\n👋 Interrupted. Exiting...")

if __name__ == "__main__":
    if not os.path.exists(TTY_DEVICE):
        print(f"⚠️ TTY device {TTY_DEVICE} not found.")
    else:
        main()


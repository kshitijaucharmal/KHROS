import subprocess
import os
import curses
import time
from typing import List

TTY_DEVICE = "/dev/ttyUSB0"

class TextEditor:
    def __init__(self, stdscr):
        self.stdscr = stdscr
        self.lines: List[str] = [""]
        self.current_line = 0
        self.current_col = 0
        self.status = ""
        self.mode = "NORMAL"  # NORMAL or INSERT
        self.history: List[List[str]] = []  # Stack to store previous states
        self.tab_size = 4  # Tab size in spaces
        self.variables = {}  # Dictionary to store variables

        # Define command categories and their colors
        self.command_colors = {
            "gpio": curses.color_pair(5),  # Red for GPIO commands
            "counter": curses.color_pair(6),  # Yellow for counter commands
            "system": curses.color_pair(3),  # Green for system commands
            "test": curses.color_pair(4),  # Blue for test commands
            "control": curses.color_pair(1),  # Cyan for control structures
            "keyword": curses.color_pair(2),  # Yellow for Python keywords
            "variable": curses.color_pair(4),  # Blue for variables
        }

        # Define available commands with descriptions
        self.available_commands = {
            "gpio": {
                "gpio_on": "Turn GPIO pin on",
                "gpio_off": "Turn GPIO pin off",
                "reset_gpio": "Reset GPIO configuration"
            },
            "counter": {
                "hex_counter": "Display counter in hexadecimal",
                "left_counter": "Shift counter left",
                "right_counter": "Shift counter right"
            },
            "system": {
                "level": "Show current privilege level",
                "board_name": "Display board name",
                "timer_resolution": "Show timer resolution",
                "mmu": "Display MMU status",
                "driver": "List loaded drivers",
                "irq_handler": "Show interrupt handlers",
                "kernel_heap": "Display kernel heap info"
            },
            "test": {
                "test": "Run system test"
            },
            "control": {
                "if": "Conditional execution",
                "for": "Loop execution",
                "end": "End control block"
            }
        }

        # Define Python keywords and operators
        self.python_keywords = {
            "in", "range", "True", "False", "None", "and", "or", "not",
            "is", "==", "!=", "<", ">", "<=", ">=", "+", "-", "*", "/", "%",
            "let"  # Add 'let' as a keyword for variable assignment
        }

        self.commands = {
            ord('i'): self.enter_insert_mode,
            ord('a'): self.enter_insert_mode_after,
            ord('h'): self.move_left,
            ord('j'): self.move_down,
            ord('k'): self.move_up,
            ord('l'): self.move_right,
            ord('o'): self.insert_line_below,
            ord('O'): self.insert_line_above,
            ord('x'): self.delete_char,
            ord('d'): self.delete_line,
            ord('s'): self.send_current_line,
            ord('S'): self.send_all_lines,
            ord(':'): self.command_mode,
            ord('A'): self.move_to_end_and_insert,
            ord('I'): self.move_to_start_and_insert,
            ord('u'): self.undo,
            curses.KEY_ENTER: self.new_line,
            curses.KEY_BACKSPACE: self.backspace,
        }

    def enter_insert_mode(self):
        self.mode = "INSERT"
        self.update_status()

    def enter_insert_mode_after(self):
        self.mode = "INSERT"
        self.current_col = min(self.current_col + 1, len(self.lines[self.current_line]))
        self.update_status()

    def move_left(self):
        if self.current_col > 0:
            current_indent = self.get_indent_level(self.lines[self.current_line])
            if self.current_col == current_indent:
                # Jump to previous indentation level
                self.current_col = max(0, current_indent - 4)
            else:
                self.current_col -= 1

    def move_right(self):
        if self.current_col < len(self.lines[self.current_line]):
            current_indent = self.get_indent_level(self.lines[self.current_line])
            if self.current_col == current_indent - 4:
                # Jump to next indentation level
                self.current_col = current_indent
            else:
                self.current_col += 1

    def move_up(self):
        if self.current_line > 0:
            self.current_line -= 1
            # Adjust column to stay within indentation
            current_indent = self.get_indent_level(self.lines[self.current_line])
            self.current_col = min(self.current_col, len(self.lines[self.current_line]))
            if self.current_col < current_indent:
                self.current_col = current_indent

    def move_down(self):
        if self.current_line < len(self.lines) - 1:
            self.current_line += 1
            # Adjust column to stay within indentation
            current_indent = self.get_indent_level(self.lines[self.current_line])
            self.current_col = min(self.current_col, len(self.lines[self.current_line]))
            if self.current_col < current_indent:
                self.current_col = current_indent

    def insert_line_below(self):
        self.lines.insert(self.current_line + 1, "")
        self.current_line += 1
        self.current_col = 0
        self.mode = "INSERT"
        self.update_status()

    def insert_line_above(self):
        self.lines.insert(self.current_line, "")
        self.current_col = 0
        self.mode = "INSERT"
        self.update_status()

    def delete_char(self):
        if self.current_col < len(self.lines[self.current_line]):
            self.lines[self.current_line] = (
                self.lines[self.current_line][:self.current_col] +
                self.lines[self.current_line][self.current_col + 1:]
            )

    def delete_line(self):
        if len(self.lines) > 1:
            del self.lines[self.current_line]
            if self.current_line >= len(self.lines):
                self.current_line = len(self.lines) - 1
            self.current_col = min(self.current_col, len(self.lines[self.current_line]))

    def handle_tab(self):
        """Handle tab key press"""
        if self.mode == "INSERT":
            # Insert spaces for tab
            spaces = " " * self.tab_size
            self.lines[self.current_line] = (
                self.lines[self.current_line][:self.current_col] +
                spaces +
                self.lines[self.current_line][self.current_col:]
            )
            self.current_col += self.tab_size

    def auto_indent_control(self, line: str) -> int:
        """Calculate indentation for control structures"""
        words = line.split()
        if not words:
            return 0

        cmd = words[0].lower()
        if cmd in ["if", "for", "else"]:
            return self.get_indent_level(line) + self.tab_size
        elif cmd == "end":
            return max(0, self.get_indent_level(line) - self.tab_size)
        return self.get_indent_level(line)

    def new_line(self):
        """Insert a new line with proper indentation"""
        if self.mode == "INSERT":
            # Get current line content
            current_line = self.lines[self.current_line]

            # Split current line at cursor
            before_cursor = current_line[:self.current_col]
            after_cursor = current_line[self.current_col:]

            # Update current line
            self.lines[self.current_line] = before_cursor

            # Calculate indentation for new line
            indent = self.auto_indent_control(before_cursor)
            new_line = " " * indent + after_cursor

            # Insert new line
            self.lines.insert(self.current_line + 1, new_line)
            self.current_line += 1
            self.current_col = indent

            self.update_status()
            self.refresh()

    def backspace(self):
        """Handle backspace with indentation support"""
        if self.mode == "INSERT":
            if self.current_col > 0:
                # If at start of indentation, delete entire level
                current_indent = self.get_indent_level(self.lines[self.current_line])
                if self.current_col == current_indent and current_indent > 0:
                    self.lines[self.current_line] = " " * (current_indent - 4) + self.lines[self.current_line][current_indent:]
                    self.current_col = current_indent - 4
                else:
                    # Normal backspace
                    self.lines[self.current_line] = (
                        self.lines[self.current_line][:self.current_col - 1] +
                        self.lines[self.current_line][self.current_col:]
                    )
                    self.current_col -= 1
            elif self.current_line > 0:
                # Join with previous line
                prev_line_len = len(self.lines[self.current_line - 1])
                self.lines[self.current_line - 1] += self.lines[self.current_line]
                del self.lines[self.current_line]
                self.current_line -= 1
                self.current_col = prev_line_len

    def send_current_line(self):
        line = self.lines[self.current_line].strip()
        if line:
            send_command_to_tty(line)
            self.status = f"✅ Sent: {line}"
        else:
            self.status = "❌ Empty line, nothing to send"

    def process_variable_assignment(self, line: str) -> bool:
        """Process variable assignment and return True if successful"""
        if line.startswith("let "):
            try:
                # Parse variable assignment: let x = 5
                parts = line[4:].strip().split("=")
                if len(parts) != 2:
                    raise ValueError("Invalid variable assignment syntax")

                var_name = parts[0].strip()
                value = eval(parts[1].strip(), self.variables)

                # Store the variable
                self.variables[var_name] = value
                self.status = f"✅ Set {var_name} = {value}"
                return True
            except Exception as e:
                self.status = f"❌ Error in variable assignment: {str(e)}"
                return False
        return False

    def evaluate_condition(self, condition: str) -> bool:
        """Evaluate a condition using variables"""
        try:
            # Replace variable names with their values
            for var_name, value in self.variables.items():
                condition = condition.replace(var_name, str(value))
            return eval(condition)
        except Exception as e:
            self.status = f"❌ Error in condition: {str(e)}"
            return False

    def process_control_structures(self, lines: List[str]) -> List[str]:
        """Process if statements, else if, else, and for loops in the code"""
        processed_lines = []
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            if not line:
                i += 1
                continue

            # Process variable assignment
            if self.process_variable_assignment(line):
                i += 1
                continue

            # Process if statement
            if line.startswith("if "):
                condition = line[3:].strip()
                try:
                    # Evaluate condition using variables
                    if self.evaluate_condition(condition):
                        # If true, include lines until else/else if/end
                        i += 1
                        while i < len(lines):
                            next_line = lines[i].strip()
                            if next_line.startswith("else if ") or next_line == "else" or next_line == "end":
                                break
                            processed_lines.append(lines[i])
                            i += 1
                    else:
                        # If false, skip until else/else if/end
                        while i < len(lines):
                            next_line = lines[i].strip()
                            if next_line.startswith("else if ") or next_line == "else" or next_line == "end":
                                break
                            i += 1
                except Exception as e:
                    self.status = f"❌ Error in if condition: {str(e)}"
                    return []

            # Process else if statement
            elif line.startswith("else if "):
                condition = line[8:].strip()
                try:
                    if self.evaluate_condition(condition):
                        # If true, include lines until else/end
                        i += 1
                        while i < len(lines):
                            next_line = lines[i].strip()
                            if next_line == "else" or next_line == "end":
                                break
                            processed_lines.append(lines[i])
                            i += 1
                    else:
                        # If false, skip until else/end
                        while i < len(lines):
                            next_line = lines[i].strip()
                            if next_line == "else" or next_line == "end":
                                break
                            i += 1
                except Exception as e:
                    self.status = f"❌ Error in else if condition: {str(e)}"
                    return []

            # Process else statement
            elif line == "else":
                # Include lines until end
                i += 1
                while i < len(lines):
                    next_line = lines[i].strip()
                    if next_line == "end":
                        break
                    processed_lines.append(lines[i])
                    i += 1

            # Process for loop
            elif line.startswith("for "):
                try:
                    # Parse for loop: for i in range(5)
                    parts = line[4:].strip().split(" in ")
                    if len(parts) != 2 or not parts[1].startswith("range("):
                        raise ValueError("Invalid for loop syntax")

                    var_name = parts[0].strip()
                    range_expr = parts[1][6:-1]  # Remove 'range(' and ')'
                    range_args = [int(arg.strip()) for arg in range_expr.split(",")]

                    # Get loop body
                    loop_body = []
                    i += 1
                    while i < len(lines) and not lines[i].strip().startswith("end"):
                        loop_body.append(lines[i])
                        i += 1

                    # Execute loop
                    for value in range(*range_args):
                        # Set loop variable
                        self.variables[var_name] = value
                        processed_lines.extend(loop_body)

                except Exception as e:
                    self.status = f"❌ Error in for loop: {str(e)}"
                    return []
                i += 1

            # Skip end statements
            elif line == "end":
                i += 1

            # Regular command
            else:
                processed_lines.append(line)
                i += 1

        return processed_lines

    def send_all_lines(self):
        # Process control structures first
        processed_lines = self.process_control_structures(self.lines)
        if not processed_lines:
            return

        for i, line in enumerate(processed_lines):
            line = line.strip()
            if line:
                send_command_to_tty(line)
                # Truncate the line if it's too long for display
                max_line_length = 40  # Maximum length to display
                display_line = line[:max_line_length] + "..." if len(line) > max_line_length else line
                self.status = f"✅ Sent line {i + 1}: {display_line}"
                self.refresh()
                time.sleep(0.5)  # Small delay between commands
        self.status = "✅ All lines sent"

    def command_mode(self):
        max_y, max_x = self.stdscr.getmaxyx()
        # Leave one character space at the end to prevent curses errors
        self.stdscr.addstr(max_y - 1, 0, ":" + " " * (max_x - 2))
        self.stdscr.move(max_y - 1, 1)
        command = ""
        while True:
            c = self.stdscr.getch()
            if c == curses.KEY_ENTER or c == 10:
                break
            elif c == 27:  # ESC
                command = ""
                break
            elif c >= 32 and c <= 126:  # Only handle printable ASCII
                command += chr(c)
                # Don't write if we're at the edge of the screen
                if len(command) < max_x - 2:
                    self.stdscr.addch(c)

        if command == "q":
            return True
        elif command == "w":
            self.status = "Save not implemented"
        return False

    def update_status(self, message: str = ""):
        if message:
            self.status = message
        else:
            self.status = f"Mode: {self.mode} | Line: {self.current_line + 1}/{len(self.lines)} | Col: {self.current_col}"

    def show_help_window(self):
        """Display a help window with all available commands"""
        max_y, max_x = self.stdscr.getmaxyx()

        # Create a new window for help with safe boundaries
        help_height = min(25, max_y - 4)  # Leave some margin
        help_width = min(70, max_x - 4)   # Leave some margin
        help_y = (max_y - help_height) // 2
        help_x = (max_x - help_width) // 2

        help_win = curses.newwin(help_height, help_width, help_y, help_x)
        help_win.box()

        # Add title
        title = "Available Commands"
        help_win.addstr(0, (help_width - len(title)) // 2, title, curses.A_BOLD)

        # Display commands by category
        y = 2
        for category, commands in self.available_commands.items():
            # Category header
            if y < help_height - 1:
                help_win.addstr(y, 2, f"{category.upper()}:", self.command_colors[category] | curses.A_BOLD)
                y += 1

            # Commands in category
            for cmd, desc in commands.items():
                if y < help_height - 2:  # Leave space for close message
                    # Truncate description if too long
                    max_desc_len = help_width - 25  # Leave space for command and formatting
                    truncated_desc = desc[:max_desc_len] + "..." if len(desc) > max_desc_len else desc

                    help_win.addstr(y, 4, f"{cmd}", self.command_colors[category])
                    help_win.addstr(y, 20, f"- {truncated_desc}", curses.color_pair(7))
                    y += 1

            y += 1  # Add space between categories

        # Add close instruction
        if y < help_height - 1:
            help_win.addstr(help_height - 1, 2, "Press any key to close", curses.A_DIM)

        help_win.refresh()

        # Wait for key press with timeout
        self.stdscr.timeout(-1)  # Disable timeout temporarily
        self.stdscr.getch()      # Wait for any key press
        self.stdscr.timeout(100) # Restore timeout

        # Clear the help window
        help_win.clear()
        help_win.refresh()
        del help_win

    def highlight_command(self, line: str, y: int, x: int) -> int:
        """Highlight commands in the line and return the next x position"""
        # First print the indentation spaces
        indent = len(line) - len(line.lstrip())
        self.stdscr.addstr(y, x, " " * indent, curses.color_pair(7))
        x += indent

        # Get the content without indentation
        content = line.lstrip()
        if not content:
            return x

        words = content.split()
        cmd = words[0].lower()

        # Check for control structures first
        if cmd in ["if", "for", "else", "end"] or (len(words) > 1 and words[0].lower() == "else" and words[1].lower() == "if"):
            if len(words) > 1 and words[0].lower() == "else" and words[1].lower() == "if":
                # Handle "else if" as a single control structure
                self.stdscr.addstr(y, x, "else if", self.command_colors["control"])
                x += len("else if")
                if len(words) > 2:
                    # Highlight the rest of the line with Python keywords
                    rest = " " + " ".join(words[2:])
                    self.highlight_python_keywords(y, x, rest)
                    x += len(rest)
            else:
                # Handle other control structures
                self.stdscr.addstr(y, x, words[0], self.command_colors["control"])
                x += len(words[0])
                if len(words) > 1:
                    # Highlight the rest of the line with Python keywords
                    rest = " " + " ".join(words[1:])
                    self.highlight_python_keywords(y, x, rest)
                    x += len(rest)
            return x

        # Check other commands
        for category, commands in self.available_commands.items():
            if cmd in commands:
                # Highlight the command
                self.stdscr.addstr(y, x, words[0], self.command_colors[category])
                x += len(words[0])
                # Add space and rest of the line
                if len(words) > 1:
                    rest = " " + " ".join(words[1:])
                    self.highlight_python_keywords(y, x, rest)
                    x += len(rest)
                return x

        # If no command found, print normally with Python keyword highlighting
        self.highlight_python_keywords(y, x, content)
        return x + len(content)

    def highlight_python_keywords(self, y: int, x: int, text: str):
        """Highlight Python keywords in the given text"""
        # Split text into words and spaces
        parts = []
        current_word = ""
        for char in text:
            if char == " ":
                if current_word:
                    parts.append(current_word)
                    current_word = ""
                parts.append(" ")
            else:
                current_word += char
        if current_word:
            parts.append(current_word)

        current_x = x
        for part in parts:
            if part == " ":
                self.stdscr.addstr(y, current_x, " ")
                current_x += 1
            else:
                # Check if word is a Python keyword
                if part in self.python_keywords:
                    self.stdscr.addstr(y, current_x, part, self.command_colors["keyword"])
                # Check if word is a variable
                elif part in self.variables:
                    self.stdscr.addstr(y, current_x, part, self.command_colors["variable"])
                else:
                    self.stdscr.addstr(y, current_x, part, curses.color_pair(7))
                current_x += len(part)

    def refresh(self):
        self.stdscr.clear()
        max_y, max_x = self.stdscr.getmaxyx()

        # Define padding
        PADDING_TOP = 2
        PADDING_LEFT = 4
        PADDING_RIGHT = 4
        PADDING_BOTTOM = 2

        # Calculate available space
        available_width = max_x - PADDING_LEFT - PADDING_RIGHT
        available_height = max_y - PADDING_TOP - PADDING_BOTTOM

        # Display ASCII art header with padding
        header_art = r"""
  _  __ ____  _   _  ___  ____
 | |/ /|  _ \| | | |/ _ \/ ___|
 | ' / | |_) | |_| | | | \___ \
 | . \ |  _ <|  _  | |_| |___) |
 |_|\_\|_| \_\_| |_|\___/|____/
        """
        header_lines = header_art.strip().split('\n')
        for i, line in enumerate(header_lines):
            self.stdscr.addstr(i + PADDING_TOP, PADDING_LEFT, line, curses.color_pair(1) | curses.A_BOLD)

        # Add version and separator with padding
        version = "v0.1.0"
        self.stdscr.addstr(len(header_lines) + PADDING_TOP, PADDING_LEFT, version, curses.color_pair(1))
        separator = "─" * available_width
        self.stdscr.addstr(len(header_lines) + PADDING_TOP + 1, PADDING_LEFT, separator, curses.color_pair(1))

        # Calculate line number width based on total lines
        line_num_width = len(str(len(self.lines))) + 2  # +2 for the colon and space

        # Display lines with line numbers and syntax highlighting
        start_y = len(header_lines) + PADDING_TOP + 2
        for i, line in enumerate(self.lines):
            if i < available_height - 3:  # Leave space for status and help
                # Format line number with padding and color
                line_num = f"{i + 1}: ".rjust(line_num_width)
                self.stdscr.addstr(i + start_y, PADDING_LEFT, line_num, curses.color_pair(2))
                # Display line content with syntax highlighting
                self.highlight_command(line, i + start_y, PADDING_LEFT + line_num_width)

        # Display cursor with different style based on mode
        cursor_x = PADDING_LEFT + line_num_width + self.current_col
        if self.mode == "INSERT":
            self.stdscr.addch(self.current_line + start_y, cursor_x,
                            curses.ACS_CKBOARD, curses.A_REVERSE)
        else:
            self.stdscr.addch(self.current_line + start_y, cursor_x,
                            ' ', curses.A_REVERSE)

        # Display status with color and message
        status_text = self.status if self.status else f"Mode: {self.mode} | Line: {self.current_line + 1}/{len(self.lines)} | Col: {self.current_col}"
        # Truncate status text if it's too long
        if len(status_text) > available_width:
            status_text = status_text[:available_width - 3] + "..."
        self.stdscr.addstr(max_y - PADDING_BOTTOM - 1, PADDING_LEFT, status_text, curses.color_pair(3))
        self.stdscr.addstr(max_y - PADDING_BOTTOM - 1, len(status_text) + PADDING_LEFT,
                         " " * (available_width - len(status_text)), curses.color_pair(3))

        # Display enhanced help panel with command categories
        help_sections = [
            ("Move", "←↓↑→  or  hjkl"),
            ("Insert", "i:insert  a:append  o:new line"),
            ("Edit", "x:delete  d:cut line  u:undo"),
            ("Send", "s:line  S:all"),
            ("Quit", ":q")
        ]

        # Calculate total width needed
        total_width = sum(len(section[0]) + len(section[1]) + 4 for section in help_sections)

        # Center the help panel
        start_x = max(0, (available_width - total_width) // 2) + PADDING_LEFT

        # Display help sections with colors and separators
        current_x = start_x
        for i, (title, commands) in enumerate(help_sections):
            # Display section title
            self.stdscr.addstr(max_y - PADDING_BOTTOM, current_x, title, curses.color_pair(4) | curses.A_BOLD)
            current_x += len(title) + 1

            # Display commands
            self.stdscr.addstr(max_y - PADDING_BOTTOM, current_x, commands, curses.color_pair(4))
            current_x += len(commands)

            # Add separator if not the last section
            if i < len(help_sections) - 1:
                self.stdscr.addstr(max_y - PADDING_BOTTOM, current_x, " | ", curses.color_pair(4))
                current_x += 3

        # Move cursor to current position (accounting for line numbers, header, and padding)
        self.stdscr.move(self.current_line + start_y, cursor_x)
        self.stdscr.refresh()

    def save_state(self):
        """Save current state to history"""
        self.history.append(self.lines.copy())

    def undo(self):
        """Undo the last change"""
        if self.history:
            self.lines = self.history.pop()
            # Keep cursor within bounds
            self.current_line = min(self.current_line, len(self.lines) - 1)
            self.current_col = min(self.current_col, len(self.lines[self.current_line]))
            self.update_status()
            return False
        return False

    def run(self):
        self.update_status()
        self.refresh()

        # Set a very short timeout for getch to prevent ESC delay
        self.stdscr.timeout(100)  # 100ms timeout

        while True:
            c = self.stdscr.getch()

            # Skip if no key was pressed (timeout)
            if c == -1:
                continue

            # Handle arrow keys and special keys
            if c == curses.KEY_UP:
                self.move_up()
            elif c == curses.KEY_DOWN:
                self.move_down()
            elif c == curses.KEY_LEFT:
                self.move_left()
            elif c == curses.KEY_RIGHT:
                self.move_right()
            # Handle tab key
            elif c == ord('\t'):
                self.handle_tab()
            # Handle help key (changed from 'h' to '?')
            elif c == ord('?'):
                self.show_help_window()
                self.refresh()
                continue
            # Handle ESC key
            elif c == 27:  # ESC
                if self.mode == "INSERT":
                    self.mode = "NORMAL"
                    self.update_status()
                    self.refresh()
                    continue
                else:
                    # If we get another key after ESC, it might be an arrow key
                    next_key = self.stdscr.getch()
                    if next_key == -1:  # No more keys, it was just ESC
                        continue
                    # Handle arrow keys in escape sequence
                    if next_key == ord('['):
                        arrow = self.stdscr.getch()
                        if arrow == -1:  # Timeout
                            continue
                        if arrow == ord('A'):  # Up
                            self.move_up()
                        elif arrow == ord('B'):  # Down
                            self.move_down()
                        elif arrow == ord('C'):  # Right
                            self.move_right()
                        elif arrow == ord('D'):  # Left
                            self.move_left()
                    continue

            if self.mode == "INSERT":
                if c == curses.KEY_BACKSPACE:
                    self.save_state()  # Save state before backspace
                    self.backspace()
                elif c == curses.KEY_ENTER or c == 10:
                    self.save_state()  # Save state before new line
                    self.new_line()
                elif c >= 32 and c <= 126:  # Only handle printable ASCII characters
                    self.save_state()  # Save state before inserting character
                    char = chr(c)
                    self.lines[self.current_line] = (
                        self.lines[self.current_line][:self.current_col] +
                        char +
                        self.lines[self.current_line][self.current_col:]
                    )
                    self.current_col += 1
            else:
                if c in self.commands:
                    if self.commands[c]() == True:  # If command returns True, exit
                        return

            self.refresh()

    def move_to_end_and_insert(self):
        self.current_col = len(self.lines[self.current_line])
        self.mode = "INSERT"
        self.update_status()

    def move_to_start_and_insert(self):
        self.current_col = 0
        self.mode = "INSERT"
        self.update_status()

    def get_indent_level(self, line: str) -> int:
        """Get the indentation level of a line"""
        return len(line) - len(line.lstrip())

    def get_block_indent(self, line_num: int) -> int:
        """Get the indentation level for a new line in a block"""
        if line_num <= 0:
            return 0

        # Look backwards for control structure
        for i in range(line_num - 1, -1, -1):
            line = self.lines[i].strip()
            if line.startswith(("if ", "for ")):
                return self.get_indent_level(self.lines[i]) + 4
            elif line == "end":
                return max(0, self.get_indent_level(self.lines[i]) - 4)

        return 0

def send_command_to_tty(command: str):
    if not command.strip():
        return
    try:
        full_command = f'echo "{command}" > {TTY_DEVICE}'
        subprocess.run(['sudo', 'bash', '-c', full_command], check=True)
        print(f"✅ Sent: {command}")
    except subprocess.CalledProcessError as e:
        print(f"❌ Error sending command: {e}")

def check_sudo():
    """Check if the program has sudo privileges"""
    # try:
    #     # Try to read from the TTY device
    #     with open(TTY_DEVICE, 'r') as f:
    #         pass
    return True
    # except PermissionError:
    #     return False

def request_sudo():
    """Request sudo privileges"""
    try:
        # Try to run a simple command with sudo
        subprocess.run(['sudo', 'true'], check=True)
        return True
    except subprocess.CalledProcessError:
        return False

def main(stdscr):
    # Check for sudo privileges
    if not check_sudo():
        stdscr.clear()
        stdscr.addstr(0, 0, "⚠️ Sudo privileges required to access TTY device")
        stdscr.addstr(1, 0, "Requesting sudo privileges...")
        stdscr.refresh()

        if not request_sudo():
            stdscr.clear()
            stdscr.addstr(0, 0, "❌ Failed to get sudo privileges")
            stdscr.addstr(1, 0, "Press any key to exit")
            stdscr.getch()
            return

    # Initialize curses
    curses.curs_set(1)  # Show cursor
    stdscr.keypad(True)  # Enable keypad
    curses.noecho()  # Don't echo characters
    curses.cbreak()  # Don't wait for Enter

    # Initialize colors
    curses.start_color()
    curses.use_default_colors()

    # Define color pairs
    curses.init_pair(1, curses.COLOR_CYAN, -1)    # Header
    curses.init_pair(2, curses.COLOR_YELLOW, -1)  # Line numbers
    curses.init_pair(3, curses.COLOR_GREEN, -1)   # Status bar
    curses.init_pair(4, curses.COLOR_BLUE, -1)    # Help text
    curses.init_pair(5, curses.COLOR_RED, -1)     # GPIO commands
    curses.init_pair(6, curses.COLOR_YELLOW, -1)  # Counter commands
    curses.init_pair(7, curses.COLOR_WHITE, -1)   # Normal text (default color)

    # Create and run editor
    editor = TextEditor(stdscr)
    editor.run()

if __name__ == "__main__":
    try:
        curses.wrapper(main)
    except KeyboardInterrupt:
        print("\n👋 Interrupted. Exiting...")


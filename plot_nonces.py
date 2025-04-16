import pandas as pd
import matplotlib.pyplot as plt
import re
import io
import argparse
import numpy as np # For calculating the average hash rate safely
import platform # Import platform module
import subprocess # Import subprocess to run sysctl

# --- Argument Parsing ---
parser = argparse.ArgumentParser(description='Plot mining difficulty vs nonce count and time.')
parser.add_argument(
    'input_file',
    nargs='?',
    default='nonce_data.txt',
    help='Path to the input data file (default: nonce_data.txt)'
)
args = parser.parse_args()

# --- Use parsed filename ---
INPUT_FILENAME = args.input_file
OUTPUT_PLOT_FILENAME = "difficulty_vs_metrics.png" # Back to original name

# --- Get CPU Info ---
cpu_info = "Unknown CPU" # Default value
try:
    system = platform.system()
    if system == "Darwin": # macOS specific check
        try:
            # Use sysctl to get the detailed CPU brand string on macOS
            result = subprocess.run(
                ['sysctl', '-n', 'machdep.cpu.brand_string'],
                capture_output=True, text=True, check=True
            )
            cpu_info = result.stdout.strip()
        except (FileNotFoundError, subprocess.CalledProcessError, Exception) as e:
             print(f"Warning: Could not get detailed CPU info via sysctl: {e}")
             # Fallback to platform.processor() or uname on macOS if sysctl fails
             cpu_info = platform.processor()
             if not cpu_info:
                 uname_info = platform.uname()
                 cpu_info = f"{uname_info.machine} (macOS fallback)"

    elif system == "Linux":
        # On Linux, try reading /proc/cpuinfo for model name
        try:
            with open('/proc/cpuinfo') as f:
                 for line in f:
                     if line.strip().startswith('model name'):
                         cpu_info = line.split(':', 1)[1].strip()
                         break # Found the first model name
            # Fallback if 'model name' not found
            if cpu_info == "Unknown CPU":
                cpu_info = platform.processor()
                if not cpu_info:
                    uname_info = platform.uname()
                    cpu_info = f"{uname_info.machine} (Linux fallback)"
        except FileNotFoundError:
             print("Warning: /proc/cpuinfo not found.")
             cpu_info = platform.processor() or f"{platform.uname().machine} (Linux fallback)"

    elif system == "Windows":
         cpu_info = platform.processor() or f"{platform.uname().machine} (Windows fallback)" # platform.processor often works well on Windows

    else: # Other OS
         cpu_info = platform.processor()
         if not cpu_info:
              uname_info = platform.uname()
              cpu_info = f"{uname_info.machine} / {system} (fallback)"

except Exception as e:
    print(f"Warning: Error detecting CPU info: {e}")
    cpu_info = "Unknown CPU" # Ensure it has a default

data = []
# Regex updated to capture the "Avg Time" again
line_regex = re.compile(r"Difficulty:\s*(\d+),\s*Average Nonce Count:\s*([\d\.]+),\s*Avg Time:\s*([\d\.]+)\s*s,\s*Aggregate Hash Rate:\s*([\d\.]+)")

print(f"Reading data from {INPUT_FILENAME}...")
try:
    with open(INPUT_FILENAME, 'r') as f:
        for line in f:
            match = line_regex.match(line.strip())
            if match:
                try:
                    difficulty = int(match.group(1))
                    avg_nonce_count = float(match.group(2))
                    avg_time = float(match.group(3)) # Capture Avg Time
                    agg_hash_rate = float(match.group(4))
                    data.append({
                        "Difficulty": difficulty,
                        "AvgNonceCount": avg_nonce_count,
                        "AvgTime": avg_time, # Store Avg Time
                        "AggHashRate": agg_hash_rate
                    })
                except ValueError:
                    print(f"Warning: Could not parse numbers in line: {line.strip()}")

    if not data:
         print("Error: No valid data lines found in the file. Did the Rust program run correctly?")
         print(f"Check the contents of '{INPUT_FILENAME}'. Expecting lines like:")
         # Updated example format
         print("Difficulty: 56000000000, Average Nonce Count: 886.08, Avg Time: 1.207 s, Aggregate Hash Rate: 4555.75")
         exit()

    df = pd.DataFrame(data)
    df = df.sort_values(by="Difficulty") # Sort by difficulty for plotting

    # --- Scale Difficulty by 1 million ---
    df["DifficultyMillions"] = df["Difficulty"] / 1e6

    print("Data loaded:")
    print(df)

    # Calculate overall average aggregate hash rate
    valid_hash_rates = df['AggHashRate'].replace([np.inf, -np.inf], np.nan).dropna()
    overall_avg_hash_rate = valid_hash_rates.mean() if not valid_hash_rates.empty else 0

    print(f"\nOverall Average Aggregate Hash Rate: {overall_avg_hash_rate:.2f} Nonces/s (Approx)")
    print(f"Detected CPU/System: {cpu_info}") # Print detected info

    # --- Create Two Plots ---
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(10, 10), sharex=True) # Back to 2 rows, shared x-axis
    fig.suptitle(f'Mining Metrics vs. Difficulty on {cpu_info}\n(Overall Avg. Aggregate Hash Rate: {overall_avg_hash_rate:.2f} Nonces/s)')

    # Plot 1: Difficulty vs Average Nonce Count
    ax1.scatter(df["DifficultyMillions"], df["AvgNonceCount"], label="Measured Data")
    ax1.plot(df["DifficultyMillions"], df["AvgNonceCount"], linestyle='--', alpha=0.6, label="Trend (linear assumption)")
    ax1.set_ylabel("Average Nonce Count")
    ax1.set_title("Difficulty vs. Average Nonce Count") # Can add title back if preferred
    ax1.grid(True)
    ax1.legend()
    ax1.ticklabel_format(style='plain', axis='y')

    # Plot 2: Difficulty vs Average Time per Solution
    ax2.scatter(df["DifficultyMillions"], df["AvgTime"], label="Measured Data", color='orange') # Use different color
    ax2.plot(df["DifficultyMillions"], df["AvgTime"], linestyle='--', alpha=0.6, label="Trend (linear assumption)", color='red')
    ax2.set_xlabel("Difficulty (Millions)") # Set x-label on the bottom plot
    ax2.set_ylabel("Average Time per Solution (s)")
    ax2.set_title("Difficulty vs. Average Time per Solution")
    ax2.grid(True)
    ax2.legend()
    # Format x-axis (applies to both since sharex=True)
    ax1.ticklabel_format(style='plain', axis='x') # Use plain for scaled x-axis


    plt.tight_layout(rect=[0, 0.03, 1, 0.93]) # Adjust layout slightly for title

    # Save the plot
    plt.savefig(OUTPUT_PLOT_FILENAME)
    print(f"Plot saved to {OUTPUT_PLOT_FILENAME}")

    # Show the plot
    plt.show()

except FileNotFoundError:
    print(f"Error: Input file '{INPUT_FILENAME}' not found.")
    print("Please run the Rust example first and redirect its output:")
    print("cargo run --release --example nonce_counter -p qpow-math > nonce_data.txt")
    print("Or provide the correct filename as an argument:")
    print("python plot_nonces.py your_data_file.txt")
except Exception as e:
    print(f"An error occurred: {e}")

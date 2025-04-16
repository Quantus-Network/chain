import pandas as pd
import matplotlib.pyplot as plt
import re
import io
import argparse
import numpy as np # For calculating the average hash rate safely

# --- Argument Parsing ---
parser = argparse.ArgumentParser(description='Plot mining difficulty vs nonce count.')
parser.add_argument(
    'input_file',
    nargs='?',
    default='nonce_data.txt',
    help='Path to the input data file (default: nonce_data.txt)'
)
args = parser.parse_args()

# --- Use parsed filename ---
INPUT_FILENAME = args.input_file
OUTPUT_PLOT_FILENAME = "difficulty_vs_nonce_count.png" # Renamed output file

data = []
# Updated Regex to capture the new format
line_regex = re.compile(r"Difficulty:\s*(\d+),\s*Average Nonce Count:\s*([\d\.]+),\s*Total Time:\s*([\d\.]+),\s*Aggregate Hash Rate:\s*([\d\.]+)")

print(f"Reading data from {INPUT_FILENAME}...")
try:
    with open(INPUT_FILENAME, 'r') as f:
        for line in f:
            match = line_regex.match(line.strip())
            if match:
                try:
                    difficulty = int(match.group(1))
                    avg_nonce_count = float(match.group(2))
                    total_time = float(match.group(3)) # Can still capture if needed, but not plotting
                    agg_hash_rate = float(match.group(4))
                    data.append({
                        "Difficulty": difficulty,
                        "AvgNonceCount": avg_nonce_count,
                        "AggHashRate": agg_hash_rate
                    })
                except ValueError:
                    print(f"Warning: Could not parse numbers in line: {line.strip()}")

    if not data:
         print("Error: No valid data lines found in the file. Did the Rust program run correctly?")
         print(f"Check the contents of '{INPUT_FILENAME}'. Expecting lines like:")
         print("Difficulty: 56000000000, Average Nonce Count: 745.18, Total Time: 7.783, Aggregate Hash Rate: 4787.08")
         exit()

    df = pd.DataFrame(data)
    df = df.sort_values(by="Difficulty") # Sort by difficulty for plotting

    # --- Scale Difficulty by 1 million ---
    df["DifficultyMillions"] = df["Difficulty"] / 1e6

    print("Data loaded:")
    print(df) # Will now show the DifficultyMillions column too

    # Calculate overall average aggregate hash rate (handling potential NaNs/Infs if any sample failed badly)
    valid_hash_rates = df['AggHashRate'].replace([np.inf, -np.inf], np.nan).dropna()
    overall_avg_hash_rate = valid_hash_rates.mean() if not valid_hash_rates.empty else 0

    print(f"\nOverall Average Aggregate Hash Rate: {overall_avg_hash_rate:.2f} H/s (Approx)") # Note: This is hashes/sec across all cores

    # --- Create Single Plot ---
    fig, ax1 = plt.subplots(1, 1, figsize=(10, 6)) # Adjusted for single plot

    # Add average hash rate to title
    fig.suptitle(f'Difficulty vs. Average Nonce Count\n(Overall Avg. Aggregate Hash Rate: {overall_avg_hash_rate:.2f} H/s)')

    # Plot: Difficulty vs Average Nonce Count (Using scaled difficulty)
    ax1.scatter(df["DifficultyMillions"], df["AvgNonceCount"], label="Measured Data")
    ax1.plot(df["DifficultyMillions"], df["AvgNonceCount"], linestyle='--', alpha=0.6, label="Trend (linear assumption)")
    ax1.set_xlabel("Difficulty (Millions)") # Updated x-axis label
    ax1.set_ylabel("Average Nonce Count")
    # ax1.set_title("Difficulty vs. Average Nonce Count") # Title included in fig.suptitle
    ax1.grid(True)
    ax1.legend()
    ax1.ticklabel_format(style='plain', axis='y') # Keep plain format for nonce count
    ax1.ticklabel_format(style='plain', axis='x') # Use plain format for scaled x-axis


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

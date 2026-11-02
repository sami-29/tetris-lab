// Ticket 06 spike — C++ port of the same bitboard beam kernel as the Rust
// version, so the language question gets a number on identical games.
//
// Faithful to `TetrisSolver.py` in the same ways the Rust kernel is: beam
// schedule (1,3,7,25), state -> rotation -> column order, stable descending
// sort, dead `_W_LINES` term, one eval per spawn-legal placement.
//
//   g++ -O3 -std=c++20 -o prototype/cpp/t06_bench.exe prototype/cpp/t06_bench.cpp
//   ./prototype/cpp/t06_bench.exe --games data.txt --goal 8 --out rows.jsonl
//
// Throwaway code. No production use.

#include <algorithm>
#include <bit>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

namespace {

constexpr int BOARD_W = 10;
constexpr int BOARD_H = 20;

constexpr int W_HEIGHT = -51;
constexpr int W_HOLES = -35;
constexpr int W_BUMPINESS = -18;

struct Rot {
    int width;
    int height;
    uint8_t cols[4];
};

// Same literals as tetromino_data.py: piece order I J L O S T Z, rotation
// order exactly as written there (rotation order breaks beam ties).
using Rows = std::vector<const char*>;

const std::vector<std::vector<Rows>>& shapes() {
    static const std::vector<std::vector<Rows>> kShapes = {
        {{"1111"}, {"1", "1", "1", "1"}},                                        // I
        {{"100", "111"}, {"11", "10", "10"}, {"111", "001"}, {"01", "01", "11"}},  // J
        {{"001", "111"}, {"10", "10", "11"}, {"111", "100"}, {"11", "01", "01"}},  // L
        {{"11", "11"}},                                                          // O
        {{"011", "110"}, {"10", "11", "01"}},                                     // S
        {{"010", "111"}, {"10", "11", "10"}, {"111", "010"}, {"01", "11", "01"}},  // T
        {{"110", "011"}, {"01", "11", "10"}},                                     // Z
    };
    return kShapes;
}

std::vector<std::vector<Rot>> build_rots() {
    std::vector<std::vector<Rot>> out(7);
    for (int p = 0; p < 7; ++p) {
        for (const Rows& rows : shapes()[p]) {
            const int height = static_cast<int>(rows.size());
            const int width = static_cast<int>(std::strlen(rows[0]));
            Rot rot{width, height, {0, 0, 0, 0}};
            for (int row = 0; row < height; ++row) {
                for (int c = 0; c < width; ++c) {
                    if (rows[row][c] == '1') {
                        rot.cols[c] |= static_cast<uint8_t>(1u << (height - 1 - row));
                    }
                }
            }
            out[p].push_back(rot);
        }
    }
    return out;
}

struct Game {
    uint32_t seed;
    uint32_t board[BOARD_W];
    std::vector<uint8_t> seq;
};

inline int bitlen(uint32_t w) { return w == 0 ? 0 : 32 - std::countl_zero(w); }

inline int evaluate(const uint32_t (&board)[BOARD_W]) {
    int sum_h = 0, holes = 0, bump = 0, prev = -1;
    for (int c = 0; c < BOARD_W; ++c) {
        const int h = bitlen(board[c]);
        sum_h += h;
        holes += h - std::popcount(board[c]);
        if (prev >= 0) bump += std::abs(prev - h);
        prev = h;
    }
    return W_HEIGHT * sum_h + W_HOLES * holes + W_BUMPINESS * bump;
}

inline uint32_t clear_rows(uint32_t w, uint32_t full) {
    for (int b = 31; b >= 0; --b) {
        if (full & (1u << b)) {
            const uint32_t lower = b == 0 ? 0u : (w & ((1u << b) - 1));
            const uint32_t upper = (w >> (b + 1)) << b;
            w = lower | upper;
        }
    }
    return w;
}

struct Cand {
    int score;
    int lines;
    uint32_t board[BOARD_W];
};

struct State {
    int lines;
    uint32_t board[BOARD_W];
};

struct Solve {
    bool success;
    uint32_t evals;
};

Solve beam_search(const Game& game, const std::vector<std::vector<Rot>>& rots, int width,
                  int goal) {
    uint32_t evals = 0;
    std::vector<State> beam(1);
    beam[0].lines = 0;
    std::memcpy(beam[0].board, game.board, sizeof(game.board));
    std::vector<State> next;
    std::vector<Cand> cands;
    next.reserve(static_cast<size_t>(width) * 40);
    cands.reserve(static_cast<size_t>(width) * 40);

    for (const uint8_t piece : game.seq) {
        cands.clear();
        for (const State& state : beam) {
            for (const Rot& rot : rots[piece]) {
                const int s_start = BOARD_H - rot.height;
                for (int col = 0; col + rot.width <= BOARD_W; ++col) {
                    int s_min = 0;
                    for (int i = 0; i < rot.width; ++i) {
                        const int tz = std::countr_zero(static_cast<uint32_t>(rot.cols[i]));
                        const int need = bitlen(state.board[col + i]) - tz;
                        if (need > s_min) s_min = need;
                    }
                    if (s_min > s_start) continue;
                    evals += 1;
                    const int s = s_min < 0 ? 0 : s_min;

                    Cand cand;
                    std::memcpy(cand.board, state.board, sizeof(cand.board));
                    for (int i = 0; i < rot.width; ++i) {
                        cand.board[col + i] |= static_cast<uint32_t>(rot.cols[i]) << s;
                    }

                    uint32_t full = 0xFFFFFFFFu;
                    for (int c = 0; c < BOARD_W; ++c) full &= cand.board[c];
                    full &= 0x000FFFFFu;  // only the 20 real rows
                    int lines = 0;
                    if (full != 0) {
                        lines = std::popcount(full);
                        for (int c = 0; c < BOARD_W; ++c) cand.board[c] = clear_rows(cand.board[c], full);
                    }

                    const int new_lines = state.lines + lines;
                    if (new_lines >= goal) return {true, evals};

                    cand.score = evaluate(cand.board);
                    cand.lines = new_lines;
                    cands.push_back(cand);
                }
            }
        }
        if (cands.empty()) return {false, evals};

        if (static_cast<int>(cands.size()) > width) {
            std::stable_sort(cands.begin(), cands.end(),
                             [](const Cand& a, const Cand& b) { return a.score > b.score; });
            cands.resize(width);
        }

        next.clear();
        for (const Cand& c : cands) {
            State st;
            st.lines = c.lines;
            std::memcpy(st.board, c.board, sizeof(st.board));
            next.push_back(st);
        }
        beam.swap(next);
    }
    return {false, evals};
}

Solve solve(const Game& game, const std::vector<std::vector<Rot>>& rots, int goal) {
    static const int kSchedule[4] = {1, 3, 7, 25};
    uint32_t total = 0;
    for (const int width : kSchedule) {
        const Solve r = beam_search(game, rots, width, goal);
        total += r.evals;
        if (r.success) return {true, total};
    }
    return {false, total};
}

std::vector<Game> parse_games(const std::string& path) {
    std::ifstream in(path);
    if (!in) {
        std::fprintf(stderr, "cannot open %s\n", path.c_str());
        std::exit(2);
    }
    std::vector<Game> games;
    std::string line;
    while (std::getline(in, line)) {
        if (line.rfind("seed ", 0) == 0) {
            Game g;
            g.seed = static_cast<uint32_t>(std::stoul(line.substr(5)));
            std::memset(g.board, 0, sizeof(g.board));
            for (int r = 0; r < BOARD_H; ++r) {
                std::getline(in, line);
                for (int c = 0; c < BOARD_W; ++c) {
                    if (line[c] == '1') g.board[c] |= 1u << (BOARD_H - 1 - r);
                }
            }
            std::getline(in, line);
            for (const char ch : line) {
                g.seq.push_back(static_cast<uint8_t>(std::string("IJLOSTZ").find(ch)));
            }
            games.push_back(std::move(g));
        }
    }
    return games;
}

}  // namespace

int main(int argc, char** argv) {
    std::string games_path = "prototype/data/games_goal8_p40_seed0-999.txt";
    std::string out_path;
    std::string check_path;
    int goal = 8;
    int limit = 0;
    for (int i = 1; i < argc; ++i) {
        const std::string flag = argv[i];
        const std::string val = i + 1 < argc ? argv[i + 1] : "";
        if (flag == "--games") { games_path = val; ++i; }
        else if (flag == "--goal") { goal = std::stoi(val); ++i; }
        else if (flag == "--out") { out_path = val; ++i; }
        else if (flag == "--check") { check_path = val; ++i; }
        else if (flag == "--limit") { limit = std::stoi(val); ++i; }
        else { std::fprintf(stderr, "unknown flag %s\n", flag.c_str()); return 2; }
    }

    const auto rots = build_rots();
    std::vector<Game> games = parse_games(games_path);
    if (limit > 0 && static_cast<int>(games.size()) > limit) games.resize(limit);
    if (games.empty()) {
        std::fprintf(stderr, "no games\n");
        return 2;
    }

    struct Row { uint32_t seed; bool ok; uint32_t evals; double ms; };
    std::vector<Row> rows;
    rows.reserve(games.size());
    const auto t_all = std::chrono::steady_clock::now();
    for (const Game& g : games) {
        const auto t0 = std::chrono::steady_clock::now();
        const Solve r = solve(g, rots, goal);
        const double ms =
            std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
        rows.push_back({g.seed, r.success, r.evals, ms});
    }
    const double wall_ms =
        std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t_all).count();

    uint64_t evals_total = 0;
    int successes = 0;
    std::vector<double> times;
    for (const Row& r : rows) {
        evals_total += r.evals;
        successes += r.ok ? 1 : 0;
        times.push_back(r.ms);
    }
    std::sort(times.begin(), times.end());
    const double mean = [&] {
        double s = 0;
        for (const double t : times) s += t;
        return s / static_cast<double>(times.size());
    }();
    const auto pct = [&](double p) { return times[static_cast<size_t>(p * (times.size() - 1) + 0.5)]; };

    if (!out_path.empty()) {
        std::ofstream out(out_path);
        for (const Row& r : rows) {
            out << "{\"solver\":\"cpp-u32\",\"seed\":" << r.seed << ",\"goal\":" << goal
                << ",\"pieces\":" << games[0].seq.size() << ",\"success\":" << (r.ok ? "true" : "false")
                << ",\"evals\":" << r.evals << ",\"ms\":" << r.ms << "}\n";
        }
    }

    std::printf(
        "{\"solver\":\"cpp-u32\",\"games\":%zu,\"goal\":%d,\"success_rate\":%.4f,"
        "\"evals_total\":%llu,\"evals_per_s\":%.1f,\"games_per_s\":%.2f,\"ms_median\":%.4f,"
        "\"ms_mean\":%.4f,\"ms_p90\":%.4f,\"ms_total\":%.1f}\n",
        rows.size(), goal, static_cast<double>(successes) / static_cast<double>(rows.size()),
        static_cast<unsigned long long>(evals_total), static_cast<double>(evals_total) / wall_ms * 1000.0,
        static_cast<double>(rows.size()) / wall_ms * 1000.0, pct(0.5), mean, pct(0.9), wall_ms);

    if (!check_path.empty()) {
        std::ifstream in(check_path);
        int bad_success = 0, bad_evals = 0;
        std::string line;
        std::vector<uint32_t> seeds;
        std::vector<int> ok_expected, evals_expected;
        while (std::getline(in, line)) {
            const auto grab = [&](const char* key) {
                const std::string pat = std::string("\"") + key + "\":";
                const size_t pos = line.find(pat);
                if (pos == std::string::npos) return std::string();
                size_t start = pos + pat.size();
                while (start < line.size() && line[start] == ' ') ++start;
                size_t end = start;
                while (end < line.size() && line[end] != ',' && line[end] != '}') ++end;
                return line.substr(start, end - start);
            };
            seeds.push_back(static_cast<uint32_t>(std::stoul(grab("seed"))));
            ok_expected.push_back(grab("success") == "true" ? 1 : 0);
            evals_expected.push_back(std::stoi(grab("evals")));
        }
        for (const Row& r : rows) {
            for (size_t i = 0; i < seeds.size(); ++i) {
                if (seeds[i] != r.seed) continue;
                if (ok_expected[i] != (r.ok ? 1 : 0)) ++bad_success;
                if (static_cast<uint32_t>(evals_expected[i]) != r.evals) ++bad_evals;
                break;
            }
        }
        std::printf("{\"check\":\"%s\",\"success_mismatches\":%d,\"eval_mismatches\":%d}\n",
                    check_path.c_str(), bad_success, bad_evals);
    }
    return 0;
}

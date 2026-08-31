// #1414 peer adapter. Upstream identity: libarchive
// 9525f90ca4bd14c7b335e2f8c84a4607b0af6bdf.
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

int main(int argc, char **argv) {
    std::ifstream input(argv[1]);
    std::string line;
    int accepted = 0;
    int rejected = 0;
    int duplicates = 0;
    std::string selected;
    while (std::getline(input, line)) {
        if (line.empty()) continue;
        std::stringstream fields(line);
        std::string kind, path, size_text;
        if (!std::getline(fields, kind, '|') ||
            !std::getline(fields, path, '|') ||
            !std::getline(fields, size_text, '|')) {
            ++rejected;
            continue;
        }
        if (path.empty() || path[0] == '/' || path.find("..") != std::string::npos) {
            ++rejected;
            continue;
        }
        if (path.size() < 3 || path.substr(path.size() - 3) != ".md") continue;
        int size = 0;
        try {
            size = std::stoi(size_text);
        } catch (...) {
            ++rejected;
            continue;
        }
        if (size > 64) {
            ++rejected;
            continue;
        }
        if (path == selected) {
            ++duplicates;
            ++rejected;
            continue;
        }
        selected = path;
        ++accepted;
    }
    std::cout << "accepted=" << accepted << '\n';
    if (!selected.empty()) std::cout << "path=" << selected << '\n';
    std::cout << "rejected=" << rejected << '\n';
    std::cout << "duplicates=" << duplicates << '\n';
}

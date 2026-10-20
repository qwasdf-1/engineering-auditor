"""Seeded recall cases in Go; see `cases.py` for the shapes."""

GO = {
    "repeated_sort_in_loop": {
        "direct": {"service/service.go": """\
package service

import "sort"

func inspect(requests []Request, candidates []int) {
	for _, request := range requests {
		sort.Ints(candidates)
		consume(request, candidates[0])
	}
}
"""},
        "helper": {"service/service.go": """\
package service

import "sort"

func refresh(candidates []int) {
	sort.Ints(candidates)
}

func inspect(requests []Request, candidates []int) {
	for _, request := range requests {
		refresh(candidates)
		consume(request, candidates[0])
	}
}
"""},
        "cross_file": {
            "service/service.go": """\
package service

func inspect(requests []Request, candidates []int) {
	for _, request := range requests {
		refresh(candidates)
		consume(request, candidates[0])
	}
}
""",
            "service/ranking.go": """\
package service

import "sort"

func refresh(candidates []int) {
	sort.Ints(candidates)
}
""",
        },
        "negative": {"service/service.go": """\
package service

import "sort"

func inspect(requests []Request, candidates []int) {
	sort.Ints(candidates)
	for _, request := range requests {
		consume(request, candidates[0])
	}
}
"""},
    },
    "n_plus_one_query_in_loop": {
        "direct": {"service/service.go": """\
package service

func load(ids []int, repo Repository) {
	for _, id := range ids {
		consume(repo.FindByID(id))
	}
}
"""},
        "helper": {"service/service.go": """\
package service

func loadOne(repo Repository, id int) Row {
	return repo.FindByID(id)
}

func load(ids []int, repo Repository) {
	for _, id := range ids {
		consume(loadOne(repo, id))
	}
}
"""},
        "cross_file": {
            "service/service.go": """\
package service

func load(ids []int, repo Repository) {
	for _, id := range ids {
		consume(loadOne(repo, id))
	}
}
""",
            "service/store.go": """\
package service

func loadOne(repo Repository, id int) Row {
	return repo.FindByID(id)
}
""",
        },
        "negative": {"service/service.go": """\
package service

func load(ids []int, repo Repository) {
	consume(repo.FindMany(ids))
}
"""},
    },
    "per_item_external_io": {
        "direct": {"service/service.go": """\
package service

import "net/http"

func fetchAll(urls []string) {
	for _, url := range urls {
		response, _ := http.Get(url)
		consume(response)
	}
}
"""},
        "helper": {"service/service.go": """\
package service

import "net/http"

func details(url string) *http.Response {
	response, _ := http.Get(url)
	return response
}

func fetchAll(urls []string) {
	for _, url := range urls {
		consume(details(url))
	}
}
"""},
        "cross_file": {
            "service/service.go": """\
package service

func fetchAll(urls []string) {
	for _, url := range urls {
		consume(details(url))
	}
}
""",
            "service/remote.go": """\
package service

import "net/http"

func details(url string) *http.Response {
	response, _ := http.Get(url)
	return response
}
""",
        },
        "negative": {"service/service.go": """\
package service

import "net/http"

func fetchAll(batch string) {
	response, _ := http.Get(batch)
	consume(response)
}
"""},
    },
    "loop_invariant_regex_compilation": {
        "direct": {"service/service.go": """\
package service

import "regexp"

func inspect(items []string) {
	for _, item := range items {
		pattern := regexp.MustCompile("^fixed$")
		consume(pattern, item)
	}
}
"""},
        "helper": {"service/service.go": """\
package service

import "regexp"

func matcher() *regexp.Regexp {
	return regexp.MustCompile("^fixed$")
}

func inspect(items []string) {
	for _, item := range items {
		pattern := matcher()
		consume(pattern, item)
	}
}
"""},
        "cross_file": {
            "service/service.go": """\
package service

func inspect(items []string) {
	for _, item := range items {
		pattern := matcher()
		consume(pattern, item)
	}
}
""",
            "service/patterns.go": """\
package service

import "regexp"

func matcher() *regexp.Regexp {
	return regexp.MustCompile("^fixed$")
}
""",
        },
        "negative": {"service/service.go": """\
package service

import "regexp"

func inspect(items []string) {
	pattern := regexp.MustCompile("^fixed$")
	for _, item := range items {
		consume(pattern, item)
	}
}
"""},
    },
    "loop_artifact_rewrite": {
        "direct": {"service/service.go": """\
package service

import "os"

func rewrite(paths []string) {
	for _, path := range paths {
		raw, _ := os.ReadFile(path)
		_ = os.WriteFile("report.json", raw, 0o644)
	}
}
"""},
        "helper": {"service/service.go": """\
package service

import "os"

func save(raw []byte) {
	_ = os.WriteFile("report.json", raw, 0o644)
}

func rewrite(paths []string) {
	for _, path := range paths {
		raw, _ := os.ReadFile(path)
		save(raw)
	}
}
"""},
        "cross_file": {
            "service/service.go": """\
package service

import "os"

func rewrite(paths []string) {
	for _, path := range paths {
		raw, _ := os.ReadFile(path)
		save(raw)
	}
}
""",
            "service/report.go": """\
package service

import "os"

func save(raw []byte) {
	_ = os.WriteFile("report.json", raw, 0o644)
}
""",
        },
        "negative": {"service/service.go": """\
package service

import "os"

func rewrite(paths []string) {
	raw, _ := os.ReadFile("input")
	for _, path := range paths {
		consume(path, raw)
	}
}
"""},
    },
}

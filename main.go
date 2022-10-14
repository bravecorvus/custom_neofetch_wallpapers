package main

import (
	"log"
	"math/rand"
	"net/http"
	"os"
	"time"

	cron "github.com/robfig/cron/v3"
)

var (
	filepaths chan string
)

func init() {
}

func main() {
	var dir string
	if len(os.Args) != 2 {
		dir = os.Getenv("HOME") + "/custom_neofetch_wallpapers/img"
	} else {
		dir = os.Args[1]
	}

	getPicturePaths(dir)

	c := cron.New()
	c.AddFunc("@hourly", func() { getPicturePaths(dir) })
	c.Start()

	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		filepath := <-filepaths
		defer func() {
			filepaths <- filepath
		}()
		w.Write([]byte(filepath))
	})
	log.Fatal(http.ListenAndServe(":7777", nil))
}

func getPicturePaths(dir string) {

	if string(dir[len(dir)-1]) != "/" {
		dir += "/"
	}

	files, err := os.ReadDir(dir)
	if err != nil {
		panic(`os.ReadDir("` + dir + `") Exception: ` + err.Error())
	}

	filepaths = make(chan string, len(files)-1)

	rand.Seed(time.Now().UnixNano())
	rand.Shuffle(len(files), func(i, j int) { files[i], files[j] = files[j], files[i] })

	for _, file := range files {
		if file.Name() == ".DS_Store" {
			continue
		}
		filepaths <- dir + file.Name()
	}
}

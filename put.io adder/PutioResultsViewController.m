//
//  PutioResultsViewController.m
//  put.io adder
//
//  Created by Nico on 05/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import "PutioResultsViewController.h"

@implementation PutioResultsViewController

@synthesize files, tableView;

//- (void)viewDidLoad {
//    [super viewDidLoad];
//    // Do view setup here.
//}


- (NSInteger)numberOfRowsInTableView:(NSTableView *)tableView
{
    NSLog(@"%lu", (unsigned long)self.files.count);
    return self.files.count;
}


- (id)tableView:(NSTableView *)aTableView objectValueForTableColumn:(NSTableColumn *)tableColumn row:(NSInteger)row
{
    if ([tableColumn.identifier isEqualToString:@"name"])
    {
        return [[self.files objectAtIndex:row] valueForKey: @"name"];
    }
    
    NSURL *icon = [NSURL URLWithString:[[self.files objectAtIndex: row] valueForKey: @"icon"]];
    return [[NSImage alloc] initWithContentsOfURL: icon];
}

@end